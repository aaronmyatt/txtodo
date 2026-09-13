# Nested-ref sync to a fresh device + `todo.sh -d` on a sub-list — M5

Plan M5 "Acceptance", specs/ref-directories.md rules 11 and 12. Wires M4 sync machinery (done) to
M5 nested refs.

## Whole-tree sync to a fresh device (rule 11)

Syncing a workspace with nested refs to a fresh device reproduces the whole tree. Discovery is by
walking, not following tags (rule 11), so the fresh device must end up with the same directories,
the same `todo.txt`/`done.txt`/`notes.md` bytes, and the same ids. Build on the M4 loopback pair
(`tasks/sync-loopback-converge`): same harness, a nested-ref fixture instead of a flat file.

## `todo.sh -d <ref>/todo.cfg ls` on a sub-list (rule 12)

The vendored `todo.sh` (M2 parity harness) pointed at a ref directory lists the sub-list like any
other file. This is the differential-oracle half of `sub` (`cli-ref-commands`): `txtodo sub 2 ls`
and `todo.sh -d <ref>/todo.cfg ls` must agree.

## Fixture is shared by copy

The nested-ref fixture (parent → child → grandchild, with `done.txt` and `notes.md` in the middle
level) is needed here and in `test-m5-acceptance`. Copy it into each slice's test dir; do not
create a shared fixture module (constitution §7).

## Tests

- Two daemons, nested-ref fixture: after pairing, the fresh device's tree and every file's bytes
  match the source.
- `todo.sh -d <ref>/todo.cfg ls` lists the sub-list, agreeing with `txtodo sub`.

## As built (2026-09-13, agent) — whole-tree-sync half only

Per this session's own brief, the `todo.sh -d`/`txtodo sub` half of this task belongs to
`cli-ref-commands` and was not touched (not verified here either way — this pass could not find an
existing "As built" section in that task's own notes confirming it, but was instructed to trust
that claim and not redo it).

`crates/txtodo-daemon/tests/nested_ref_sync.rs`,
`fresh_device_reproduces_the_whole_nested_ref_tree`: two real `txtodod` processes; device A starts
already holding the fixture (parent `todo.txt` with `ref:child` → `child/` with `todo.txt`
(`ref:grandchild`), `done.txt`, `notes.md` → `child/grandchild/todo.txt`), device B starts on a
totally empty temp dir. Paired via the same `DebugSetGroupKey` test-only seam
`sync-loopback-converge` uses. Polls B's **on-disk** bytes (not the daemon's `GetFile` view — the
task notes' own claim is about the real directory tree) for every file except `child/notes.md`
against A's, bounded by a 5 s deadline (headroom, not a target — this task states no ms/s budget of
its own).

- **Measured: 389-498 ms** across 6 consecutive runs, no flakes, for the whole 4-file tree (parent
  `todo.txt`, `child/todo.txt`, `child/done.txt`, `child/grandchild/todo.txt`) to reach a
  fresh device, including directory creation at two levels of depth
  (`child/`, `child/grandchild/`) that did not exist on B at all beforehand. Slower than
  `sync-loopback-converge`'s sub-2 ms single-file edit — expected, since this is a first-contact
  multi-file catch-up sync through discovery + a fresh connection, not a resync of an already-paired
  session — but nowhere close to the 5 s deadline.
- **No source changes were needed in `txtodo-daemon`'s sync engine itself.** The op log's
  file-agnostic design (an `Op` carries its own `file` field; `Heads`/`Want`/`OriginRange` operate
  over a device's whole op range, not per file) plus `lan_apply.rs`'s existing
  `get_or_create_actor`/`ensure_parent_dir` (built during `sync-lan-transport`'s daemon-wiring pass,
  specifically with this task in mind) already handled multiple files and arbitrary directory depth
  correctly on first try. This task's real work was the test harness addition below, not new
  product code.
- **Harness addition**: `crates/txtodo-daemon/tests/support/mod.rs` gained
  `Daemon::start_with_seeded_group_tree(files: &[(&str, &str)], mode, group_id)` (device A needs to
  start already holding a multi-file tree, not one root `todo.txt`) and a private `write_tree`
  helper that both this and the existing `start_full`/`start_with_seeded_group_tree` now share (no
  second copy of the write-files loop). `disk_file(name: &str)` already took an arbitrary relative
  path — no change needed there; it is what this test polls.
- **`notes.md` is excluded from the convergence assertion — a real, pre-existing, two-layered gap,
  not a corner cut for this test.** See `nested_ref_sync.rs`'s own module doc for the full trace;
  summary: (1) a `notes.md` written straight to disk never becomes an `Op` at all —
  `NotesActor`/`NotesRegistry` are opened lazily, only via the `GetNotes`/`EditNotes` RPCs, and
  nothing at daemon startup diffs a pre-existing on-disk `notes.md` into a seed op the way
  `FileActor::recover` does for `todo.txt`/`done.txt`; (2) even if it did, `Workspace::register()`
  refuses to build an actor for a notes document (`Ok(false)`, no error), so `lan_apply.rs`'s
  `get_or_create_actor` on a fresh receiving device would find no actor after a successful-looking
  `register()` call and silently drop that op's commit — no warning logged. `child/notes.md` is
  still written into the fixture (matching this task's own stated shape, and proving the walker at
  least *sees* it without choking), but this test does not claim it syncs, because it does not.
  Flagged for a human as a real gap in the notes-sync pipeline — unrelated to `sync-lan-transport`'s
  own LAN wiring, and not something this task's brief (wire LAN transport, test convergence) owns
  fixing.
- Every task line in the fixture carries a valid, pre-formed `id:` tag
  (`TaskId::new(Ulid::from_u128(...))`, matching the corrected-fixture pattern from `sync-bench-m4`)
  so tagged-mode adoption on device A mints nothing extra — the bytes B is compared against are
  exactly what device A's own disk holds after adoption, not the literal fixture strings, so any
  adoption-side normalisation is captured rather than assumed away.
