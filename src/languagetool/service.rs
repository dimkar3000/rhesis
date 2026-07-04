use cxx_qt_lib::QString;

use crate::interop::bridge::ffi::Recommendation;

#[derive(Debug, Clone, Default)]
pub struct Message(pub QString);
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Suggestion(pub Vec<Recommendation>);
