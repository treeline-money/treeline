//! Treeline CLI — public library surface for integration tests.
//!
//! The CLI is primarily a binary (`tl`). This `lib` target re-exports
//! command modules so integration tests can exercise server code
//! (e.g. the hub HTTP router) without spawning a subprocess.

pub mod commands;
pub mod output;
