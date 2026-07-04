use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use log::error;
use tokio::task::JoinHandle;

use crate::interop::bridge::ffi::Recommendation;

#[derive(Default)]
pub struct CustomHighlighterRust {
    pub(crate) recommendations: Vec<Recommendation>,

    pub(crate) handle: Option<JoinHandle<()>>,
}

impl Drop for CustomHighlighterRust {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.abort();
        }
    }
}

impl CustomHighlighterRust {
    pub fn highlight_block(&self) -> Vec<(i32, i32, QString)> {
        self.recommendations
            .iter()
            .map(|r| {
                let color = QString::from(r.color.clone());
                let start = r.range.start;
                let length = r.range.length;
                (start, length, color)
            })
            .collect()
    }

    pub fn start_message_thread(
        &mut self,
        helper: &mut crate::interop::bridge::ffi::AsyncMessagingHelper,
        qt_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::CustomHighlighter>,
    ) {
        let mut receiver = helper.suggestion_receiver.clone();

        self.handle = Some(tokio::spawn(async move {
            loop {
                match receiver.changed().await {
                    Ok(_) => (),
                    Err(e) => error!("Error while waiting on the receiver: {e:?}"),
                };

                // Clone the receiver to move inside the thread
                let receiver = receiver.clone();

                match qt_thread.queue(move |mut highlighter| {
                    let suggestions = receiver.borrow().0.clone();
                    highlighter.as_mut().rust_mut().recommendations = suggestions;
                    highlighter.as_mut().rehighlight();
                }) {
                    Ok(_) => (),
                    Err(e) => error!("Error while queuing work to ui thread: {e:?}"),
                };
            }
        }));
    }
}
