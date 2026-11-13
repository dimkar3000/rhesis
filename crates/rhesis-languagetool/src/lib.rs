//! LanguageTool worker library:
//! This library contains all the structures needed to manage the languagetool server

pub mod client;
pub mod completion_provider;
pub mod constants;
pub mod dto;
pub mod service;
pub mod worker;
pub mod worker_status;

mod color_rules;
mod helpers;
