# 0001 — Use Rust for everything below the UI

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 001; do not relitigate)

## Context
One parser, formatter and sync engine must run on macOS, Linux, Windows, iOS, Android, web and terminal, with identical byte-level behaviour. Every client that re-implements the grammar is a source of drift.

## Decision
We will write every layer below the UI in Rust: core, query, model, store, CRDT, sync, daemon, MCP, CLI. Platform UIs bind to it through `txtodo-ffi` (uniffi, wasm-bindgen, cbindgen).

## Consequences
- Good: one core, many bindings; `no_std` core; one grammar everywhere.
- Bad: FFI surface to maintain; mobile build toolchains are heavier.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Go or TypeScript core with per-platform ports: the ports drift; no `no_std` story for the parser.
- Native per platform: seven grammars.
