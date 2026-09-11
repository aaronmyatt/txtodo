//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

pub mod clock;
mod fields;
pub mod reconcile;
pub mod state;
pub mod textedit;

#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod state_tests;
