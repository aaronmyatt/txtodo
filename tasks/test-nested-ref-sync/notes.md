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
