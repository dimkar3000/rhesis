
use rhesis_protocol::Recommendation;

pub use crate::worker::{LanguageToolWorker, LanguageToolWorkerEvent, WorkerHandles};
pub use rhesis_protocol::WorkerStatus;

#[derive(Debug, Clone)]
pub enum ServiceMessage {
    Suggestion(ServiceSuggestion),
    StatusChanged(WorkerStatus),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServiceSuggestion(pub Vec<Recommendation>);
