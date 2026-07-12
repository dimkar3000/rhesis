use cxx_qt_lib::QString;

use crate::interop::recommendation::Recommendation;

#[derive(Debug, Clone)]
pub enum Message {
    Suggestion(QString),
    // Get the new colors to use from the settings,
    UpdateColors(Vec<(QString, QString)>),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Suggestion(pub Vec<Recommendation>);
