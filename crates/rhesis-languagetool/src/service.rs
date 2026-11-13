use rhesis_protocol::recommendation::Recommendation;

pub use crate::completion_provider::CompletionProvider;
pub use crate::worker::{LanguageToolWorker, LanguageToolWorkerEvent, WorkerHandles};
pub use crate::worker_status::WorkerStatus;

#[derive(Debug, Clone)]
pub enum ServiceMessage {
    Suggestion(ServiceSuggestion),
    StatusChanged(WorkerStatus),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServiceSuggestion(pub Vec<Recommendation>);
