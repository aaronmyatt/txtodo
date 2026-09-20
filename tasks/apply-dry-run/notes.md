# apply-dry-run

## Goal

Decided 2026-09-20 (option A): an agent previews a batch as a unified diff before committing, and
the daemon computes it: a `dry_run` flag on `Apply` returns the diff the batch would make. It runs
the same code as the real apply, so the preview cannot drift from the result. The other choice (B)
made the MCP crate re-implement the edits; it cannot link `txtodo-core`, so it would drift.

## Design

- The dry run goes through the real mutation path, including `RequireBase` and `base_hash`
  preconditions, and stops just before the commit. Nothing is written: no op, no file, no HLC tick.
- The diff is unified, with the workspace-relative path in the headers.
- A refusal (unknown task, bad edit, failed precondition) comes back as a structured error with the
  line number and the spec rule, the same in a dry run and a real apply.
- Today `todo_batch` with `dry_run: true` never calls `Apply` (`grpc_write.rs`, `backend.rs`). That
  stub goes.

## Known gaps

- The existing test proves a dry run makes no RPC, not that a live daemon's store hash stays put
  (its own note says so). The last line closes that.
