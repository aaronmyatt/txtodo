//! MCP tools, resources, prompts (plan M6, design §6.3-6.4). `McpBackend` (`backend.rs`) is the
//! seam: schemas (`schema.rs`) and transports (`transport.rs`) never touch the daemon directly,
//! only through it. `grpc_backend.rs` is the one implementation, a gRPC client of `txtodod` —
//! see its module doc for why even the daemon-hosted HTTP transport dials itself rather than
//! reaching into actor state directly.
#![forbid(unsafe_code)]

pub mod backend;
mod backend_args;
mod doc;
pub mod error;
pub mod global_socket;
pub mod grpc_backend;
mod grpc_batch;
mod grpc_convert;
mod grpc_dry_run;
pub use grpc_convert::set_default_workspace;
mod grpc_hygiene;
mod grpc_move;
mod grpc_notes;
mod grpc_read;
mod grpc_write;
mod parse;
mod prompts;
mod resources;
pub mod schema;
mod tools;
mod tools_read;
mod tools_write;
pub mod transport;

#[cfg(test)]
mod grpc_write_tests;
