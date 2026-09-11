# Protobuf and tonic gRPC on the socket: ListFiles GetFile Watch Apply History Undo Checkout

ADR 0006: define once in protobuf, serve gRPC on the local socket only; the REST mirror comes later.
ADR 0010: socket lives at `<workspace>/.txtodo/txtodod.sock`. Ref: https://docs.rs/tonic ·
https://docs.rs/prost · https://protobuf.dev/programming-guides/proto3/

## Service
```proto
service Txtodo {
  rpc ListFiles(ListFilesRequest) returns (ListFilesResponse);      // FilePath + hash per document
  rpc GetFile(GetFileRequest) returns (FileContents);               // bytes + hash
  rpc Watch(WatchRequest) returns (stream Change);                  // file, new hash, op summaries
  rpc Apply(ApplyRequest) returns (ApplyResponse);                  // repeated Mutation, Principal
  rpc History(HistoryRequest) returns (HistoryResponse);            // file?, task?, limit
  rpc Undo(UndoRequest) returns (ApplyResponse);
  rpc Checkout(CheckoutRequest) returns (FileContents);             // file, at (RFC 3339)
}
message Mutation { oneof kind { Add add = 1; Complete complete = 2; Edit edit = 3; Move move = 4; Delete delete = 5; } }
```
Mutations are intent-level (line number or task id + what); the daemon turns them into ops.
`Apply` carries the `Principal` (User now; Agent in M6 adds token id).

## Generated code
`tonic-build` output is committed under `src/generated/` (constitution §6: generated artifact,
committed alone, diff-budget exempt). Regeneration is `just proto`; CI diffs it. Requires `protoc`
on the runner (`arduino/setup-protoc` action) — or `protoc-bin-vendored` to avoid it; pick the
vendored crate if the licence passes `cargo deny`.

## Socket serving
```rust
// https://docs.rs/tonic/latest/tonic/transport/server/struct.Server.html#method.serve_with_incoming
let uds = tokio::net::UnixListener::bind(&sock)?;
Server::builder().add_service(TxtodoServer::new(svc)).serve_with_incoming(UnixListenerStream::new(uds)).await?;
```
Bounded: tonic `concurrency_limit_per_connection(MAX_INFLIGHT_RPCS)` and a `timeout`.
