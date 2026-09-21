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

## As built (2026-09-21)

- Proto `8082b49`: `ApplyRequest.dry_run` (6), `ApplyResponse.diff` (5).
- Daemon `00a33a5`: `actor_apply.rs` plans a batch once (`plan_batch`) and either commits it or, for a dry run, returns `Preview` with a unified diff (`unified_diff.rs`, no new dependency). The clock is ticked on a copy. A refusal carries `x-txtodo-error-line` and `x-txtodo-error-rule` metadata, the same for a dry run and a real apply.
- A `TaskRef` with line 0 and an id names the task by id alone, because a batch's earlier mutations shift line numbers the later ones cannot know.
- MCP: `grpc_dry_run.rs`, one dry-run `Apply` per file; `status()` maps the metadata into `McpError.line` and `spec_rule`.

Known gaps:

- A dry run of `Replace` or of a cross-file `Move` is refused; `todo_move` in a batch is refused.
- Two ops on one task in a batch are previewed from the pre-batch text of that task.
- An `Add`'s minted id in the diff is not the one a real apply would mint.
- Only NoLine, Blank, Stale, NotATask and IdChanged carry a line or rule.
