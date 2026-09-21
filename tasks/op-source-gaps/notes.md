# op-source-gaps

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`6c94283` stamps each op's source and `f84d986` / `1f526be` surface it in `txtodo log` and the
desktop activity lists. One mutation path was missed, and it is not in the known-gaps list.

## The gap

`crates/txtodo-daemon/src/apply_route.rs:25,66-79` — `route_apply` takes `source` and forwards it on
both non-`Move` branches, but the `Ok([Mutation::Move { .. }])` arm calls
`apply_move(&path, task, to, principal)` with no `source`.
`crates/txtodo-daemon/src/move_coordinator.rs:36,39,75,131,144` then uses `ActorHandle::apply`
(source `None`) for all four of its commits.

So a `todo_move` / `txtodo mv` records a blank source while every other client mutation records
`cli` / `mcp` / `tui` / `desktop`. `txtodo log` and the desktop activity pane show a hole.
`tasks/op-source/notes.md` lists Undo, Resolve and ref-dir ops as known gaps — Move is not among
them, so this one is an oversight rather than a deferral.

## Duplication landed in the same commit

`crates/txtodo-daemon/src/activity.rs:42-56` and `crates/txtodo-daemon/src/history.rs:257-273` both
compute `min(seq)` / `max(seq)` over the rows, call `store.sources_between`, and fall back to
`unwrap_or_default()` — the same block written twice. They are the only two consumers of
`sources_between`, so any later change (a span cap, a different empty-row policy, mapping `None` to
`"unknown"`) will land in one and drift from the other.

## Test

`source: "desktop"` is set in two places (`apps/desktop/src-tauri/src/commands.rs:275`,
`bin/e2e_bridge.rs:304`) and rendered in two components, and nothing asserts it end to end.
`apps/desktop/src-tauri/tests/new_rpcs.rs:305`'s `op_log_drains_the_stream_into_a_vec` already
applies a mutation and reads the op log — one `assert_eq!(entry.source, "desktop")` there pins the
field down. As it stands a `..Default::default()` refactor that drops it is invisible.

## Related, not filed here

`tasks/complete-to-bottom/todo.txt:5` closed with "daemon mode still sends Edit plus MoveToEnd …
so the op log says edit and move". That directly degrades this work — `txtodo log` will never show
a `complete` op from the CLI — and is filed as its own root line.
