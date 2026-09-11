//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

pub mod actor;
pub mod clock;
pub mod expected;
mod external;
mod fields;
pub mod handle;
pub mod mutation;
pub mod reconcile;
pub mod state;
pub mod textedit;
pub mod write;

#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod state_tests;
