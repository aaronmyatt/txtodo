# 0006 — Local IPC is gRPC over a unix socket or named pipe, with a generated REST/JSON mirror

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 006; do not relitigate)

## Context
Every client (CLI, TUI, desktop, editors) talks to the daemon; scripts and curl want JSON. Two hand-written schemas would drift.

## Decision
We will define the API once in protobuf (`txtodo-proto`), serve gRPC via `tonic` on the local socket only, and generate the REST/JSON mirror on loopback from the same schema (`tonic-web` or a thin axum shim).

## Consequences
- Good: one schema, two surfaces; typed clients for free in every language.
- Bad: protobuf generated code is a generated artifact to keep out of the diff budget (constitution §6).
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- REST only: streaming `Watch` gets awkward; typed clients need hand work.
- JSON-RPC over the socket: no codegen, no streaming contract.
