//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

mod activity;
pub mod actor;
mod actor_mirror;
pub mod clock;
pub mod convert;
pub mod debounce;
pub mod expected;
mod external;
pub mod fastid;
mod fields;
pub mod handle;
pub mod history;
mod import;
pub mod mirror;
mod mirror_converge;
pub mod mutation;
mod notes;
mod pairing_grpc;
mod pairing_state;
mod pairing_wire;
pub mod pidfile;
pub mod reconcile;
pub mod serve;
pub mod server;
pub mod state;
pub mod stats;
pub mod telemetry;
pub mod textedit;
mod tokens;
pub mod walker;
pub mod watch_task;
pub mod watcher;
pub mod workspace;
pub mod write;

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod mirror_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod pairing_grpc_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod state_goldens;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod workspace_tests;
