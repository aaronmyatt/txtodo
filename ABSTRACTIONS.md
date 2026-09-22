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

## 2026-09-12 — tonic-over-unix-socket connector, three copies

- Duplicated: `Endpoint::try_from("http://[::]:50051").connect_with_connector(service_fn(|_| UnixStream::connect(path).map(TokioIo::new)))`
  plus a tonic client built on the channel.
- Where: crates/txtodo-cli/src/client.rs (`Daemon::connect`), crates/txtodo-daemon/tests/grpc.rs
  (`connect`), crates/txtodo-daemon/tests/support/mod.rs (`connect`, with retries). Three copies
  in two slices; `txtodo-tui` (M10) and `txtodo-mcp` (M6) will want a fourth and fifth.
- Might become: a `connect_uds(path) -> Result<Channel>` in `txtodo-proto` (it already owns the
  generated client and depends on tonic; adding hyper-util + tower there is one place, not five).
  Wait for M6, which is the next real consumer.

## 2026-09-12 — real-daemon test harness beside the CLI test runner helper

- Duplicated: spawn `txtodod --dir <tmp>`, poll for the socket (bounded), kill on drop.
- Where: crates/txtodo-cli/tests/daemon_mode.rs (`Daemon::spawn`, locates or builds the binary),
  crates/txtodo-daemon/tests/support/mod.rs (`Daemon::start`, `CARGO_BIN_EXE_txtodod`),
  crates/txtodo-daemon/tests/crash.rs (`spawn`). Constitution §7 forbids cross-slice helpers, so
  the CLI copy stays; the two daemon copies could share `tests/support`.
- 2026-09-13 (`sync-lan-transport` daemon-wiring pass): grown, not duplicated — `tests/support/
  mod.rs`'s `Daemon::start_with_mode` now delegates to a new `start_full(todo, mode, envs)` so
  `start_with_test_hooks` (sets `TXTODO_TEST_HOOKS=1` for `DebugSetGroupKey`) is one more call
  site on the same harness, used by `tests/lan_discovery.rs`. This is the "two more" the
  `sync-loopback-converge` task notes predicted; still one shared file, no new duplication.
- 2026-09-13 (`test-nested-ref-sync` pass): grown again, same file — `start_with_seeded_group` and
  `start_full` now both go through a new private `write_tree(dir, files)`, and a new
  `start_with_seeded_group_tree(files, mode, group_id)` reuses it to start a daemon on a whole
  pre-existing multi-file/nested-directory tree instead of one root `todo.txt` (needed to seed
  device A's nested-ref fixture before spawn). `tests/nested_ref_sync.rs` is the one call site so
  far; `test-m5-acceptance` is the next likely consumer of the same fixture shape.

## 2026-09-12 — "nearest task id before index i" idiom, four copies in the daemon

- Duplicated: `state.entries()[..i].iter().rev().find_map(Entry::id)` — the `after` anchor an op
  at position `i` needs (a blank has no id, so walk back to the last task).
- Where (as of e468113): crates/txtodo-daemon/src/mutation.rs:159 (`Delete`), mutation.rs:189
  (`add_ops`, over the whole list), history.rs:90 (inverse of `Deleted`), history.rs:102 (inverse
  of `Move`). Four copies, all inside one slice.
- Became: `DocState::task_before(i)` in 46ee551 — not an extraction for its own sake but part of
  narrowing the state's surface for the Loro swap (tasks/crdt-loro-state), where a borrowed slice
  can no longer be lent out. Recorded here so the ledger shows where the idiom went; the
  reconciler (`reconcile.rs`) still has its own id-scan over `File` lines, which is a fifth shape
  on a different type and stays where it is.

## 2026-09-13 — tonic-over-unix-socket connector: three copies became eight, plus the desktop's own

- Follow-up to the 2026-09-12 entry. Its trigger ("wait for M6, the next real consumer") has
  passed: M6 landed and `txtodo-mcp` added a fourth copy instead of extracting.
- Where now: crates/txtodo-cli/src/client.rs:93, crates/txtodo-mcp/src/grpc_backend.rs:76,
  crates/txtodo-daemon/tests/{tokens.rs:28, activity.rs:26, notes_grpc.rs:25, grpc.rs:30,
  m5_acceptance.rs:26, support/mod.rs:46}. Eight in three slices. apps/desktop/src-tauri/src/
  daemon.rs is a ninth shape (lazy dial + retry, same connector underneath).
- Might become: still `txtodo_proto::connect_uds(path) -> Result<Channel>`. The M11 global socket
  (todo `daemon-global-socket`) changes what every one of these dials; that is the moment to have
  one copy, not nine.

## 2026-09-13 — ADR 0010 socket and state path spelled out in five places

- Duplicated: the literal `.txtodo/txtodod.sock` (and `STATE_DIR` beside it), each with its own
  doc comment citing ADR 0010.
- Where: crates/txtodo-cli/src/client.rs:24 (`SOCKET_REL`), crates/txtodo-mcp/src/grpc_backend.rs:26
  (`SOCKET_REL`, comment says "the same … `client.rs` uses"), crates/txtodo-daemon/src/main.rs:168,
  apps/desktop/src-tauri/src/config.rs:12+43 (`socket_path`), crates/txtodo-daemon/tests/support/
  mod.rs:87.
- Might become: one `paths` module (in `txtodo-proto`, which every dialer already depends on, or
  `txtodo-model`) owning `socket_path()` / `state_dir()`. M11's `adr-global-daemon` replaces the
  per-workspace rule with a per-user one; five copies then change in lockstep or drift. Pair this
  with the connector entry above: same task, same commit.

## 2026-09-13 — three filter/query implementations while `txtodo-query` is still two lines

- Duplicated: "does this task match these terms, and in what order do matches list".
- Where: crates/txtodo-cli/src/commands/list.rs:18 (`matches`, todo.sh substring semantics with
  `-term` exclude) and :29 (`sort_key`); crates/txtodo-mcp/src/parse.rs:118
  (`matches_minimal_query`, self-described "stand-in for the design §8 query language",
  `and`/`or` "decorative"); crates/txtodo-mcp/src/grpc_read.rs:86 (substring search, todo line 21
  notes "substring-only, no query index yet"). The M11 universal view (todo line 140: "by priority
  and @context across every registered workspace") will be the fourth, in TypeScript.
- crates/txtodo-query/src/lib.rs is a doc comment and `#![forbid(unsafe_code)]`. The boundary
  allowlist already lets `txtodo-mcp`, `txtodo-daemon` and `txtodo-ffi` depend on it.
- Might become: a design §8 subset in `txtodo-query` — operands `done pri +project @context text
  key:value`, connectives `and or not`, no relative dates yet — exposing `Query::parse`,
  `Query::matches(&Task)` and one `SortKey`. The CLI keeps todo.sh `ls` semantics by translating
  terms into it; MCP `todo_search` and the desktop filter bar call it through the daemon (or the
  WASM core via `txtodo-ffi`, already allowed). This is the abstraction that makes "one priority
  interface across all projects" one code path. See docs/questions.md Q9.

## 2026-09-13 — desktop hand-mirrors every proto message twice

- Duplicated: the wire shape of each gRPC message, once as prost Rust (generated), again as a
  `*Dto` struct, again as a TypeScript interface.
- Where: crates/txtodo-proto/proto/txtodo/v1/txtodo.proto (54 messages); apps/desktop/src-tauri/
  src/dto.rs, dto_pairing.rs, dto_tokens.rs, dto_notes.rs, dto_activity.rs (13 structs);
  apps/desktop/src/lib/daemon.ts (12 types) and src/devices/types.ts (7 types).
- Cost, measured: commit 531b6cf ("fix(desktop): mirror PairOfferResponse's new identity_mode
  field") — one proto field, three edits. M11 adds a workspace selector to every request (todo
  line 131), so every message changes at once.
- Might become: `serde::Serialize` on the prost types via `prost-build`'s `type_attribute`, and
  TypeScript generated from the same `.proto` (`protobuf-es`) or from the Rust DTOs
  (`specta`/`tauri-specta`). Do it in the same task as the M11 selector, when the churn peaks.

## 2026-09-13 — atomic temp + fsync + rename: two copies became three, in three slices

- Follow-up to the 2026-09-11 entry, which predicted the third copy would be the M8 file-carrier.
  `sync-keystore` got there first.
- Where now: crates/txtodo-cli/src/store.rs:45-64, crates/txtodo-daemon/src/write.rs:61-70,
  crates/txtodo-sync/src/keystore_file.rs:288-298.
- Three callers in three slices is the constitution's own line for a kernel extraction. The
  `txtodo-fs` idea from the first entry now has a concrete third consumer; still the human's call.

## 2026-09-13 — CLI test `Command` helper: five copies became eight files

- Follow-up to the 2026-09-11 entry, which named `tests/common/mod.rs`. Three test files added
  since chose to copy instead.
- Where now: crates/txtodo-cli/tests/{env.rs:10, add.rs:9/50/69/86, list.rs:9, hygiene.rs:11,
  todosh_parity.rs:185, daemon_mode.rs:76/286, pairing.rs:77/88, nested_ref_sync.rs:80}.
- Within one slice; nothing forbids the extraction. Worth doing before M11 adds `workspace` tests.

## 2026-09-13 — the "two real txtodod processes" fixture five blocked todo lines all want

- Duplicated intent, not yet code: todo lines 8 (`sync-loopback-converge`), 9 (integration half
  of `sync-reject-tests`), 10 (`security-m4-review`), 11 (`sync-bench-m4`), 20 (fresh-device half
  of `test-nested-ref-sync`) each stop at "needs two real txtodod processes".
- Existing spawn-and-poll pieces: crates/txtodo-daemon/tests/support/mod.rs:76 (`Daemon::start`),
  crates/txtodo-daemon/tests/crash.rs:42, crates/txtodo-cli/tests/{daemon_mode.rs, pairing.rs:88,
  nested_ref_sync.rs} — the same "spawn, wait for socket, kill on drop" the 2026-09-12 entry
  already flagged.
- Might become: `support::TwoDaemons::paired()` in crates/txtodo-daemon/tests/support returning
  two connected clients, plus a `converged_within(Duration)` assertion. One fixture, five lines
  unblocked — once docs/questions.md Q10 settles whether an agent session may run it.

## 2026-09-23 — two `txtodo.toml` readers, five root-list fetchers

- Duplicated: the daemon parses and validates `<root>/txtodo.toml` in
  crates/txtodo-daemon/src/layout_file.rs (`WorkspaceLayout::new`, crates/txtodo-model/src/layout.rs)
  and the CLI parses it again for direct-file mode in crates/txtodo-cli/src/config.rs:66
  (`root_list_name`, now with its own copy of the rules in `valid_root_list` — `txtodo-cli` may not
  depend on `txtodo-model`). Each client also fetches the root list over the `WorkspaceLayout` RPC on
  its own: crates/txtodo-cli/src/commands/layout.rs (`adopt_root_list`), crates/txtodo-tui/src/daemon.rs
  (`root_list`), crates/txtodo-mcp/src/grpc_read.rs (`file_or_root`), apps/desktop/src-tauri/src/daemon/workspace.rs
  (`root_list_for`) — four copies of "Unimplemented means todo.txt, empty means todo.txt, anything
  else is an error".
- Might become: the validation rules in a tiny shared crate (or `txtodo-workspace-paths`, which every
  client already depends on) so the CLI and the daemon share one `WorkspaceLayout::new`; and one
  `root_list(&mut TxtodoClient)` helper next to the generated client in `txtodo-proto` for the four
  fetchers. `specs/ref-directories.md` rule 2 now states the rules so the two parsers cannot drift silently.
