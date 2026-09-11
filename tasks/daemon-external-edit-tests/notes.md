# Integration tests for the 8 external-edit scenarios, exactly one write, byte-identical lines

Plan M3 acceptance, first bullet. Each test starts a real `txtodod` on a temp dir (plan: "each
starting a real daemon"), so these live in `crates/txtodo-daemon/tests/` and drive it over the
socket only — the same boundary every client uses.

## Harness
`CARGO_BIN_EXE_txtodod` (https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates)
gives the built binary. `Daemon::spawn` passes `--dir` and a `TXTODO_LOG=warn` env so test output
stays readable. Slice suites are self-contained: this support module is copied into the CLI
crate's tests if M3's CLI tests need one (constitution §7), not shared.

## Counting writes
A projection write is temp + rename, so the inode changes. Record `(ino, mtime)` before the edit;
after quiescence, count inode changes seen during polling. Polling every 20 ms could miss two
renames inside one interval, so the daemon's `Health` also reports `writes_total` — the test
asserts on that and uses the inode as a cross-check.

## Scenario notes
- **Reorder** relies on `diff_lines` producing `Move` only for id-keyed lines (M1 notes).
- **Strip ids**: content keys match every line, so ops are zero text changes plus N id assignments;
  the single write must leave every line identical except the appended ` id:` tag.
- **CRLF replace**: `parse_file` records endings per file; the reconciler compares tasks, not
  bytes, so no ops; the projection is *not* rewritten to LF — the file owner chose CRLF.
- **todo.sh do 3**: the vendored script is an external oracle invoked by path with `-d` pointing at
  a generated `todo.cfg`; it archives, so `done.txt` gets an Insert seen by its own actor.

## Determinism
No sleeps except the bounded quiescence poll. The daemon's clock is real here (the test asserts
kinds and principals, never timestamps); tasks/daemon-undo-checkout-tests injects time.

## As built (2026-09-12)
`tests/support/mod.rs` + `tests/external_edits.rs`: eight scenarios pass in ~2 s. Write counting is
`Health.writes_total` only (no inode cross-check). `todo.sh do 3` runs the CLI slice's vendored
script by path with a generated `todo.cfg`. Connect retries (bounded) cover eight daemons starting
at once on a loaded machine.
