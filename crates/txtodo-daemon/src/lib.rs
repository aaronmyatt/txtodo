//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod clock;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod expected;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
mod fields;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod handle;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod mutation;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod reconcile;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod state;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod textedit;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
pub mod write;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]

//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
#[cfg(test)]
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
mod mutation_tests;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
#[cfg(test)]
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
mod reconcile_tests;
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
#[cfg(test)]
//! txtodod internals: document state, reconciler, actor, watcher, gRPC server. Plan M3.
//! The binary in `main.rs` only wires these together; everything here is testable in-process.
#![forbid(unsafe_code)]
mod state_tests;
