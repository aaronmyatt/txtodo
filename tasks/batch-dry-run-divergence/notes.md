# batch-dry-run-divergence

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`4754867` shipped `todo_batch dry_run`, and `crates/txtodo-daemon/src/actor_apply.rs:1-4` justifies
it by saying the preview runs "the *same* planning code … so the diff it returns cannot drift from
what a real apply would write". That holds inside the actor. It does not hold at the MCP layer.

## The divergence

`crates/txtodo-mcp/src/grpc_dry_run.rs:8-10,33-90` resolves every op against the **pre-batch**
document and groups them into one `Apply` per file addressed by id.
`crates/txtodo-mcp/src/grpc_write.rs:320-346` runs the real batch as one locate-then-`Apply` RPC
**per op**, addressed by line number, re-reading between ops.

Two consequences:

- Different numbers of commits, HLC ticks and `applied` counts, so the previewed `applied` can
  legitimately not match the run.
- A batch of `todo_complete(id)` then `todo_edit(id, …)` previews the edit against the
  *un-completed* raw line. The diff shown is not the diff written — exactly the case dry_run exists
  to prevent, failing silently.

It is written down as a known limit and nothing detects it.

## Design

Two routes. Cheapest first: reject a batch whose ops name the same task id more than once — that
kills the wrong-diff case outright and costs one pass over the ops. The fuller fix is routing the
real batch through the same grouped-by-id plan the dry run builds, which also makes the batch one
commit instead of N. If neither is worth it, the tool description has to say the preview is
per-op-independent, because right now `actor_apply.rs`'s comment promises otherwise.

`cee4e00` (a `TaskRef` can name a task by id alone) is what makes the grouped plan possible — but
note `peek_line` does not honour it yet, so a cross-file `Move` inside a batch still needs a line
number. That is filed as its own root line.

## Unrelated, same file family

`crates/txtodo-daemon/src/unified_diff.rs:36-41` emits
`\ line endings or the final newline changed` when only the trailing newline changed, which is not
a unified-diff token, and never emits `\ No newline at end of file`. The module doc claims this is
"the format `git diff` and `patch` read", so piping the dry-run diff into `git apply` either fails
or silently gains a trailing newline. Either emit the standard marker or drop the claim.

The LCS/hunk arithmetic itself is correct and bounded by `MAX_LCS_CELLS`.
