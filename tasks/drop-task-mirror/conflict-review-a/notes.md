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
