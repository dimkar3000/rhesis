//! LanguageTool worker library:
//! This library contains all the structures needed to manage the languagetool server

pub mod client;
pub mod constants;
pub mod dto;
pub mod service;
pub mod worker;

mod color_rules;
mod completion_provider;
mod fake_provider;
mod helpers;
