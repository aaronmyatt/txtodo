# Conflict review A: drop same-word review

## Goal
ADR 0038, conflict review A (owner, 2026-10-02): the `needs_review` flags and `Resolve`'s
Mine/Theirs/Merged go with the todo.txt mirror. The conflicts view keeps duplicate groups only
(ADR 0032). In production the flag list is already always empty: `on_import` was the only thing
that raised one, and it is gone.

## Design
- One crate per commit (the fence leases one slice at a time), each commit compiling and lint
  clean. So clients go first, then the daemon, then crdt, store, and the proto last.
- Clients first (tui, desktop, cli, mcp): they stop reading `flags`/`review` and stop calling
  `ResolveConflict`. TUI goes before desktop because the TUI parity test checks
  `specs/client-parity.toml` rows against its keymap; desktop binds none of the `conflicts.keep_*`
  rows (`keys = []`), so dropping the rows with the TUI keys breaks neither test.
- Daemon: `ResolveConflict` answers `Unimplemented` (the trait still has it until the proto line),
  `ListConflicts` sends duplicate groups only, `Change.review` stays empty.
- crdt before store: `review.rs` builds `ReviewRow`s, so it must go before the store drops the type.
- Store: drop the flag API and `CommitExtras::clear`. The table and its old rows stay (no
  migration; nothing reads them). The daemon's struct literal needs the same three-step dance the
  mirror field had (see ../notes.md, "As built (2026-10-12, store line)").
- Wire: removed fields get `reserved`, like `FILE_KIND_DONE` and `local_dir` already do. An older
  client that calls `ResolveConflict` gets `Unimplemented`; it never had a flag to resolve anyway.

## Open question (@human, proto line)
- Dropping `ResolveConflict` from the proto changes the generated tonic trait, and the daemon's
  `impl` of that method must go in the same commit (the trait has no default stubs). The fence
  denies a commit across two crates.
  - A: you allow one proto+daemon commit (delete the lease by hand, or `.claude/UNFROZEN` for it).
  - B: keep `ResolveConflict` in the proto, marked deprecated, the daemon answering
    `Unimplemented` for good. Messages and fields still go.
  - I'd take A: no dead RPC left in the contract, and it is one small commit.

## As built
- tui (2026-10-12): 05c590fc, 88cb6dfb, 9b2e6a7b, 294a5f53, four commits for the 300-line budget.
  The `r` sheet is `duplicate_sheet::draw_selected`; `conflict_sheet.rs` is gone. No line is
  read-only any more (`READ_ONLY`, `under_review` gone). `conflicts.keep_mine/theirs/merged` rows
  left the manifest; `conflicts.dismiss_banner` stays with `tui = na` until desktop drops its
  banner's ×. Not run: the TUI's `tests/e2e` (test = false target, needs a real daemon).
- desktop (2026-10-13): 0e3899a4, 7b79bf62, f36de226, 7e7777cc, cba53aa1, a2181765. TS stops
  calling the commands before the Rust drops them. The flags store, `DiffView` and `diffText` are
  gone; `list_conflicts`/`resolve` Tauri commands are gone (`list_duplicates` was already
  groups-only). The banner's × hid only the flags banner, so it and `conflicts.dismiss_banner`
  went. Checks: vitest 248, svelte-check 0, e2e tsc, clippy (with e2e-bridge), cargo test 27.
  Not run: Playwright. Gaps: the lost dirty-buffer e2e case and the unused src-tauri deps (lines
  added); the wasm `diff_text` export in txtodo-ffi has no desktop caller now.
- cli (2026-10-13): 8d26c56e. `conflicts` = list/delete/keep-newest; `conflicts_tests.rs` (flag
  rendering only) deleted. e2e conflicts tests pass against a real txtodod. Fixed the
  `chunks_exact_to_as_chunks` lint in `pri` (it failed this crate's clippy on HEAD).
  README's command table still says `conflicts [list|resolve]`: the docs line.
- mcp (2026-10-13): 25f89e1b. `todo_conflicts_list` used to return flags only, so
  it would have gone silent; it now returns duplicate groups (`[{copies: [{id, line}]}]`, oldest
  first), which it never showed before. `todo_conflicts_resolve` left the tool set and
  `EXPECTED_TOOLS`. An agent fixes a group with `todo_delete`. This changes the tool's output shape:
  an agent prompt that read `mine`/`theirs` gets groups instead.
- daemon (2026-10-13): ab97c151 (ResolveConflict → Unimplemented, its test), a4fa8b47 (Resolve
  message, `on_resolve`, `Resolution`, `NoFlag`), ec7a4958 (`Change.review`, `CommitTail`
  review/clear, `raise_flags`, `conflict_row.rs`; `conflicts()` returns groups). `actor_mirror.rs`
  builds `CommitExtras` field by field again so the store line can drop `clear`, then a daemon
  tidy commit goes back to a literal. `pb::ConflictsResponse.flags`/`pb::Change.review` are set
  empty; the proto line deletes those two lines with the fields.
- crdt (2026-10-13): 5cbcf384, ec0d4fca. `review.rs`/`review_tests.rs` gone. Row 4 of
  `specs/conflicts.md` and design §4.7 now say "char-level merge, the same on every device; no
  review flag"; its test is `edits_to_the_same_word_merge` and asserts both sides converge.
- store (2026-10-13): 0a568596 (tests), 133e4a9d (`flags.rs` → `mirror.rs`, flag API and
  `CommitExtras::clear` gone), 0da95aff (daemon literal). No migration: `review_flags` and its old
  rows stay, unread, like the old todo.txt `mirrors` rows.
- docs (2026-10-13): 415bccf0. README command table, design §4.1 text, the trial-run step 4, the desktop
  README and ADR 0038's status. Left alone: `txtodo-implementation-plan.md` (it is the plan as
  written) and `crates/txtodo-proto/CLAUDE.md` (still lists `ResolveConflict` until the proto line).
