//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

pub mod actor;
pub mod clock;
pub mod convert;
pub mod debounce;
pub mod expected;
mod external;
mod fields;
pub mod handle;
pub mod history;
pub mod mutation;
pub mod reconcile;
pub mod server;
pub mod state;
pub mod stats;
pub mod textedit;
pub mod walker;
pub mod watcher;
pub mod workspace;
pub mod write;

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod workspace_tests;
