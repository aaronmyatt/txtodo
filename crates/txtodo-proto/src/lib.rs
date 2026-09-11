//! Protobuf definitions and generated gRPC types (ADR 0006). The schema is `proto/txtodo/v1/txtodo.proto`;
//! `src/generated/` is committed output, regenerated with `cargo build -p txtodo-proto --features regen`.
#![forbid(unsafe_code)]

/// `txtodo.v1`: messages, `txtodo_server::Txtodo` service trait, `txtodo_client::TxtodoClient`.
// Generated code: documented from the .proto comments where present, rustfmt/clippy shaped by prost.
#[allow(missing_docs, clippy::all, clippy::nursery, clippy::pedantic)]
#[rustfmt::skip]
pub mod v1 {
    include!("generated/txtodo.v1.rs");
}
