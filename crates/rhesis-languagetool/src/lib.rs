//! LanguageTool worker library:
//! This library contains all the structures needed to manage the languagetool server

pub mod client;
pub mod models;
pub mod recommendation;
pub mod service;

mod completion_provider;
mod helpers;
