# Abstractions ledger

Append-only. Flag, don't extract. One entry per opportunity spotted:
what is duplicated, where (file:line per copy), and what the abstraction
might be. The human decides if and when extraction happens, as its own task.
Never edit or delete a prior entry.

---

## 2026-09-11 — txtodo-cli integration tests each build the same `Command`

- Duplicated: a helper that runs `env!("CARGO_BIN_EXE_txtodo")` in a temp dir with the environment
  scrubbed (`TXTODO_CONFIG` pointed at a missing file, `TXTODO_TODO_DIR` removed).
- Where: crates/txtodo-cli/tests/env.rs:9, tests/add.rs:8, tests/list.rs:8, tests/hygiene.rs:10,
  tests/todosh_parity.rs (`run_txtodo`). Five copies, each with a slightly different return shape.
- Might become: one `tests/common/mod.rs` with `fn txtodo(dir) -> Command`. Within one slice, so
  §7's "no cross-slice helpers" does not forbid it; still five copies is past the three-copy line.

## 2026-09-11 — txtodo-cli commands repeat read → mutate → write → print

- Duplicated: `let mut file = store::read(&ctx.paths.todo)?; … store::write(&ctx.paths.todo, &file)?;`
  around a per-line mutation, 13 times across crates/txtodo-cli/src/commands/{edit,text,fileops,
  hygiene,add}.rs.
- Might become: `store::update(&path, |file| -> Result<T>)` that reads, applies, writes only when the
  bytes changed, and returns the closure's value. Would also give every command the "write only if
  changed" behaviour `fmt` has today. Wait for M3, where daemon mode replaces this path for synced
  files: the shape may want to be "apply through core, then send or write".

## 2026-09-11 — atomic temp + fsync + rename write, second copy in the daemon

- Duplicated: write bytes to a temp file beside the target, fsync, rename over the target; read a
  missing file as empty.
- Where: crates/txtodo-cli/src/store.rs (`write`, `read`), crates/txtodo-daemon/src/write.rs
  (`write_atomic`, `read_or_empty`). Two copies; the daemon's temp name starts with `.txtodo-` so
  its own watcher ignores it, the CLI's does not need that.
- Might become: a tiny `txtodo-fs` kernel crate, or a function in `txtodo-core` behind a `std`
  feature (core is I/O-free by design, so probably not). Slices may not import each other, so the
  copy stands until the human decides. Third copy would be the M8 file-carrier transport.
