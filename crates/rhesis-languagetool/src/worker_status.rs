use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    #[default]
    Stopped,
    /// Spawning/respawning; waiting for the embedded server to come up.
    Starting,
    /// Waiting for an external LanguageTool to answer (nothing is spawned).
    Connecting,
    Started,
    /// A connect/start attempt failed; `Stopped` retries are still possible
    /// while the failure budget is not exhausted (`MAX_START_FAILURES`).
    Failed(String),
    /// The server answered at least once but requests now fail; the next
    /// successful request automatically flips back to `Started`.
    FailedToRespond(String),
}

impl WorkerStatus {
    /// Numeric code the daemon flattens onto the wire and QML reads via the
    pub fn code(&self) -> i32 {
        match self {
            WorkerStatus::Stopped => 0,
            WorkerStatus::Starting => 1,
            WorkerStatus::Started => 2,
            WorkerStatus::Failed(_) => 3,
            WorkerStatus::Connecting => 4,
            WorkerStatus::FailedToRespond(_) => 5,
        }
    }

    /// Human-readable failure detail; empty unless the status is a failure.
    pub fn reason(&self) -> String {
        match self {
            WorkerStatus::Failed(r) => r.clone(),
            WorkerStatus::FailedToRespond(r) => r.clone(),
            _ => String::new(),
        }
    }

    /// `true` whenever requests may be sent: the server either answers
    /// (Started) or is expected to answer again on the next try.
    pub fn is_started(&self) -> bool {
        matches!(
            self,
            WorkerStatus::Started | WorkerStatus::FailedToRespond(_)
        )
    }
}
