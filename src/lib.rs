//! Discrete-time fleet simulation engine driven by newline-delimited JSON.
//!
//! * [`error`]    — every failure mode as a typed enum (the behavioral contract)
//! * [`protocol`] — parsing request lines into typed commands
//! * [`model`]    — the world as plain data, and its JSON form
//! * [`engine`]   — committed / planned state, tick semantics, history

pub mod engine;
pub mod error;
pub mod model;
pub mod protocol;

pub use engine::Simulator;
pub use error::{SimError, SimResult};
