//! The single-client session pump: strict handshake, settings replay,
//! message translation and teardown bookkeeping.
//!
//! Exactly one client session may run at a time (Refactor.md §3); the
//! daemon's accept loop enforces that at connect time and this module runs
//! the conversation that follows. The socket is shared between this pump
//! (client → daemon requests) and a relay (worker → client events), so
//! every write goes through one mutex while reads get their own cloned
//! stream — a blocked read must never pin the write side of an idle relay
//! frame.
//!
//! Every exit path funnels to the same teardown: stop the relay, hand the
//! worker's message receiver back into the daemon slot (joined, so the
//! next session truly finds it there), free the single-client slot, and
//! report how the session ended to the daemon loop.

use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    thread::JoinHandle,
};

use crossbeam::channel::{unbounded, Receiver, Sender};
use interprocess::{local_socket::Stream, TryClone};

use rhesis_languagetool::service::{LanguageToolWorkerEvent, ServiceMessage};
use rhesis_protocol::constants::PROTOCOL;
use rhesis_protocol::{read_frame, write_frame, ClientMsg, DaemonMsg, Settings};

use crate::{
    config::worker_events,
    daemon::{DaemonState, SessionEnd},
};

/// Drive one client session to completion. Ownership of the single-client
/// slot was already taken by the caller; this releases it exactly once.
pub fn run_session(
    stream: Stream,
    state: Arc<DaemonState>,
    slot: Arc<Mutex<bool>>,
    end: Sender<SessionEnd>,
) {
    // Reads live on their own clone so a blocking read never holds the
    // writer mutex.
    let mut read_side = match stream.try_clone() {
        Ok(stream) => stream,
        Err(e) => {
            log::warn!("cannot clone the session stream: {e:?}");
            *slot.lock().unwrap() = false;
            let _ = end.send(SessionEnd::Done);
            return;
        }
    };
    let write_side = Arc::new(Mutex::new(stream));

    // --- strict handshake: the first frame must be `Hello` ---------------
    let handshake = match read_frame::<ClientMsg>(&mut read_side) {
        Ok(Some(ClientMsg::Hello { protocol })) if protocol == PROTOCOL => {
            let _ = mirror_frame(
                &write_side,
                &DaemonMsg::Hello {
                    protocol: PROTOCOL,
                    busy: false,
                },
            );
            true
        }
        Ok(Some(ClientMsg::Hello { protocol })) => {
            // The client still needs the daemon's version to detect the
            // mismatch and respawn a matching daemon; answer, then close.
            let _ = mirror_frame(
                &write_side,
                &DaemonMsg::Hello {
                    protocol: PROTOCOL,
                    busy: false,
                },
            );
            log::warn!("client speaks protocol {protocol}, daemon speaks {PROTOCOL}; closing");
            false
        }
        Ok(Some(message)) => {
            log::warn!("first client frame is {message:?}, not a Hello; closing");
            false
        }
        Ok(None) => {
            log::debug!("client closed before the handshake");
            false
        }
        Err(e) => {
            log::warn!("handshake failed: {e}");
            false
        }
    };

    let shutdown = if handshake {
        serve(&mut read_side, &write_side, &state)
    } else {
        false
    };
    // --- teardown (every path) -------------------------------------------
    // The relay was joined and has put the message receiver back into the
    // daemon's slot; the next session can start the moment the flag drops.
    *slot.lock().unwrap() = false;
    let _ = end.send(if shutdown {
        SessionEnd::Shutdown
    } else {
        SessionEnd::Done
    });
}

/// Post-handshake conversation; returns whether the client asked for a
/// daemon shutdown. Errors inside (client gone, worker gone) just end the
/// session — the daemon keeps accepting.
fn serve(
    read_side: &mut Stream,
    write_side: &Arc<Mutex<Stream>>,
    state: &Arc<DaemonState>,
) -> bool {
    // The daemon speaks first: full snapshot, then the current status.
    let stored = state.settings.lock().unwrap().clone();
    if !mirror_frame(write_side, &DaemonMsg::Settings(stored.clone())) {
        log::warn!("client dropped during the snapshot replay");
        return false;
    }
    let last_status = state.last_status.lock().unwrap().clone();
    if !mirror_frame(write_side, &DaemonMsg::Status(last_status)) {
        log::warn!("client dropped during the status replay");
        return false;
    }

    // Then the events that actually move the worker. On a fresh worker this
    // is the full first-handshake replay; on a reconnect within the grace
    // window it is (usually) empty — the running JVM keeps running.
    let events = {
        let applied = state.applied.lock().unwrap();
        worker_events(applied.as_ref(), &stored)
    };
    dispatch(state, events);
    *state.applied.lock().unwrap() = Some(stored);

    // Worker → client relay comes alive before any worker-moving message
    // can be accepted (statuses produced by the dispatch above buffer in
    // the worker's unbounded channel and flow as soon as the relay runs).
    let (stop_relay, stop_reading) = unbounded::<()>();
    let relay = spawn_relay(state.clone(), write_side.clone(), stop_reading);
    let Some(relay) = relay else {
        return false;
    };

    // Client → daemon translation.
    let shutdown = loop {
        let message = match read_frame::<ClientMsg>(read_side) {
            Ok(Some(message)) => message,
            Ok(None) => break false,
            Err(e) => {
                log::warn!("client frame failed, ending session: {e}");
                break false;
            }
        };
        match message {
            ClientMsg::Check { text } => {
                send(state, LanguageToolWorkerEvent::UpdateSuggestions(text));
            }
            ClientMsg::Start => send(state, LanguageToolWorkerEvent::Start),
            ClientMsg::Stop => send(state, LanguageToolWorkerEvent::Stop),
            ClientMsg::Retry => send(state, LanguageToolWorkerEvent::Retry),
            ClientMsg::SetPort(port) => {
                let mut snapshot = state.settings.lock().unwrap().clone();
                snapshot.port = port;
                let events = vec![LanguageToolWorkerEvent::SetPort(port)];
                if !apply_snapshot(write_side, state, snapshot, events) {
                    break false;
                }
            }
            ClientMsg::SwitchToRemote(address) => {
                let address = address.trim().to_string();
                let mut snapshot = state.settings.lock().unwrap().clone();
                snapshot.embedded = false;
                snapshot.external_address = address.clone();
                let events = vec![LanguageToolWorkerEvent::SwitchToRemoteServer(address)];
                if !apply_snapshot(write_side, state, snapshot, events) {
                    break false;
                }
            }
            ClientMsg::SetSettings(new_settings) => {
                // Locked order (§1): persist, mirror, then events.
                let events = {
                    let applied = state.applied.lock().unwrap();
                    worker_events(applied.as_ref(), &new_settings)
                };
                {
                    if state.persist {
                        if let Err(e) = state.store.lock().unwrap().save(&new_settings) {
                            log::warn!("settings not persisted: {e}");
                        }
                    }
                    *state.settings.lock().unwrap() = new_settings.clone();
                }
                state
                    .background
                    .store(new_settings.background, Ordering::SeqCst);
                if !mirror_frame(write_side, &DaemonMsg::Settings(new_settings.clone())) {
                    log::warn!("client dropped while mirroring the snapshot");
                    break false;
                }
                dispatch(state, events);
                *state.applied.lock().unwrap() = Some(new_settings);
            }
            ClientMsg::Shutdown => break true,
            ClientMsg::Hello { .. } => {
                log::warn!("second Hello on an established session; closing");
                break false;
            }
        }
    };

    // Teardown inside `serve` first: the relay must hand the message
    // receiver back before the caller frees the single-client slot.
    let _ = stop_relay.send(());
    if let Err(e) = relay.join() {
        log::error!("relay thread panicked: {e:?}");
    }
    shutdown
}

/// Persist (when the store is real), mirror the new snapshot, then move the
/// worker. Order mirrors the locked §1 rule so the client always sees the
/// confirmed snapshot before side effects start.
fn apply_snapshot(
    write_side: &Arc<Mutex<Stream>>,
    state: &Arc<DaemonState>,
    snapshot: Settings,
    events: Vec<LanguageToolWorkerEvent>,
) -> bool {
    if state.persist {
        if let Err(e) = state.store.lock().unwrap().save(&snapshot) {
            log::warn!("settings not persisted: {e}");
        }
    }
    *state.settings.lock().unwrap() = snapshot.clone();
    state
        .background
        .store(snapshot.background, Ordering::SeqCst);
    if !mirror_frame(write_side, &DaemonMsg::Settings(snapshot.clone())) {
        log::warn!("client dropped while mirroring the snapshot");
        return false;
    }
    dispatch(state, events);
    *state.applied.lock().unwrap() = Some(snapshot);
    true
}

fn dispatch(state: &DaemonState, events: Vec<LanguageToolWorkerEvent>) {
    for event in events {
        if let Err(e) = state.event_sender.send(event) {
            log::error!("worker event channel closed, event dropped: {e:?}");
        }
    }
}

fn send(state: &DaemonState, event: LanguageToolWorkerEvent) {
    if let Err(e) = state.event_sender.send(event) {
        log::error!("worker event channel closed, event dropped: {e:?}");
    }
}

/// Serialize one daemon frame; `false` means the client is gone.
fn mirror_frame(writer: &Arc<Mutex<Stream>>, message: &DaemonMsg) -> bool {
    let mut stream = writer.lock().unwrap();
    match write_frame(&mut *stream, message) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("client write failed: {e}");
            false
        }
    }
}

/// The worker → client half of the session: waits on the worker's message
/// channel with `select!` against its own stop signal, feeding statuses into
/// the daemon's last-status mirror and every message into the socket. On
/// exit it always hands the receiver back into the daemon's slot.
fn spawn_relay(
    state: Arc<DaemonState>,
    write_side: Arc<Mutex<Stream>>,
    stop_signal: Receiver<()>,
) -> Option<JoinHandle<()>> {
    let result = std::thread::Builder::new()
        .name("rhesis-relay".to_string())
        .spawn(move || {
            let receiver = state
                .receiver_slot
                .lock()
                .unwrap()
                .take()
                .expect("worker receiver must be present for a started session");
            loop {
                crossbeam::select! {
                    recv(stop_signal) -> _ => break,
                    recv(receiver) -> message => match message {
                        Ok(ServiceMessage::StatusChanged(status)) => {
                            *state.last_status.lock().unwrap() = status.clone();
                            let mut stream = write_side.lock().unwrap();
                            if write_frame(&mut *stream, &DaemonMsg::Status(status)).is_err() {
                                break;
                            }
                        }
                        Ok(ServiceMessage::Suggestion(suggestions)) => {
                            let mut stream = write_side.lock().unwrap();
                            if write_frame(&mut *stream, &DaemonMsg::Suggestion(suggestions.0))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(_) => {
                            log::warn!("worker message channel ended");
                            break;
                        }
                    },
                }
            }
            *state.receiver_slot.lock().unwrap() = Some(receiver);
        });
    match result {
        Ok(handle) => Some(handle),
        Err(e) => {
            log::error!("cannot spawn the relay thread: {e:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    // The pump is exercised through two tiers: the pure reconcile tests in
    // `config.rs` cover the translation rules, and the real-socket tier
    // `tests/unit/daemon_session.rs` boots the whole daemon in-process.
    // Neither needs a helper from this file, so no in-module tests exist
    // here — they would only re-run socket plumbing.
}
