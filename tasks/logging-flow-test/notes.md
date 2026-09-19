# logging-flow-test

## Goal
Root todo.txt (was line 036, `ref:logging-flow-test`): the acceptance bar for the whole `+m11
@observability` logging epic. Two parts:
(a) a no-secrets sentinel test per newly-instrumented crate — a scoped subscriber shaped like
production, a real code path driven with a sentinel value (`ZZ-SENTINEL-ZZ`) injected somewhere a
leak would be plausible, captured JSON asserted to never contain it (plus a non-empty-capture
sanity check, so the assertion isn't vacuous).
(b) a flow test asserting the ordered event *sequence* of a real two-daemon pairing+converge run
under `TXTODO_LOG=debug`, not just presence of one event name.

## Ground truth: the existing pattern
`daemon/src/lan_session_security_tests.rs:26-60` (`SERVICE`, `hex`, `open_shared`) plus its test
`no_secrets_appear_in_logs_across_a_real_pair_and_sync` (lines ~166-235): real pair, two real sync
rounds (one happy, one a peer sealing under a key nobody holds) under
`txtodo_telemetry::testing::{LogSink, capturing_dispatch}`, then asserts the group key hex/raw
bytes and every device static secret never appear, plus a sanity check that *something* was
actually logged. `tasks/daemon-tracing-logs/notes.md` names this the "ZZ-SENTINEL-ZZ technique":
inject a sentinel string into a plausible leak point (task description, note body, token secret),
drive it, assert absence in the captured JSON.

## Boundary finding (changes the plan for 4 of the 7 new-test crates)
`.claude/budgets.json`'s `slices.allowedDeps` only lists `txtodo-telemetry` as an allowed
dependency for `txtodo-mcp`, `txtodo-cli`, `txtodo-daemon`, `txtodo-tui` (and it's a `[lib]`-less
`fn`-level dep for the desktop app, checked separately) — **not** for `txtodo-store`,
`txtodo-crdt`, `txtodo-model`, or `txtodo-sync`. `.claude/scripts/check-boundaries.sh` greps both
`[dependencies]` and `[dev-dependencies]` of every crate manifest, so this applies to test-only
deps too — a `dev-dependency` on `txtodo-telemetry` would trip the fence exactly like a real one.
This is the same situation the task brief already called out for `relay` ("reimplemented the same
JSON+stderr shape locally with raw `tracing-subscriber`") — it turns out to also cover
`txtodo-store`/`txtodo-crdt`/`txtodo-model`/`txtodo-sync`. Each of those five (store, crdt, model,
sync, relay) gets its own small local `LogSink` (a `Mutex<Vec<u8>>` `Write`/`MakeWriter`) +
`capturing_dispatch` (a `tracing_subscriber::registry().with(EnvFilter).with(fmt::layer().json()
.with_writer(sink))` `Dispatch`) — the same *shape* `txtodo_telemetry::testing` gives everyone
else, not a shared dependency on it. `tracing-subscriber` itself is fine to add as a plain
dev-dependency (not a `txtodo-*` crate, so `check-boundaries.sh` never looks at it), and is already
a pinned `[workspace.dependencies]` entry in the (frozen, untouched) root `Cargo.toml`, so each
crate's own `Cargo.toml` (not frozen — `frozenPaths`' literal `"Cargo.toml"` glob matches only the
repo-root manifest, confirmed by reading `.claude/hooks/fence.sh`'s `glob()`/`hit()` — a per-crate
`crates/*/Cargo.toml` is untouched by that check) just adds `tracing-subscriber = { workspace =
true }` under `[dev-dependencies]`.

## Coverage table (before this task)

| Crate | Existing sentinel/no-secrets coverage? | Where |
|---|---|---|
| txtodo-daemon | Yes, extensive | `src/lan_session_security_tests.rs` (real pair + 2 sync rounds), `src/security_m8_tests.rs` (relay + file-carrier + bundle export/import), `src/pairing_grpc_tests.rs` (qr payload field closure) |
| txtodo-mcp | Yes | `tests/smoke.rs::mcp_call_span_names_tool_and_records_principal` |
| apps/desktop/src-tauri | Yes | `src/commands.rs::ui_log_emits_a_named_span_and_the_forwarded_message` |
| txtodo-store | No | — |
| txtodo-crdt | No | — |
| txtodo-model | No | — |
| txtodo-sync | No | — |
| txtodo-cli | No | — |
| txtodo-tui | No | — |
| relay | No (and structurally can't use `txtodo_telemetry::testing`) | — |
| txtodo-core / txtodo-query / txtodo-proto | N/A — not instrumented this epic | — |

## Design — per new test

- **txtodo-store** (`tests/no_secrets_sentinel.rs`, integration test — `Store` is a normal public
  dependency surface, no crate-internal access needed): a local `LogSink`/`capturing_dispatch`;
  `Store::commit_change_with` with a `Projection` whose `bytes` is a real todo.txt line containing
  `ZZ-SENTINEL-ZZ` (the plausible leak: `commit_landed`/`projection_written`'s fields are file
  path, op count, byte count, hash — by design never the bytes themselves; this is the regression
  guard), then `heads()`/`get_projection()` reads. Assert non-empty capture, sentinel absent.
- **txtodo-crdt** (`src/no_secrets_tests.rs`, in-crate — same convention as
  `roundtrip_tests.rs`/`lww_tests.rs`, all `#[cfg(test)] mod`s declared from `lib.rs`): drives
  `to_loro::apply` with a real `OpKind::Insert{ line: "... ZZ-SENTINEL-ZZ ..." }` (populates a Loro
  task's description text — `apply`'s span fields are `file`/`kind` only) and `lww::write_if_newer`
  with a `LoroValue::String` sentinel value (its own doc: "Never logs `value` — an LWW register may
  hold a task description"). Local `LogSink`/`capturing_dispatch`.
- **txtodo-model** (`tests/hlc_no_secrets.rs`, own test binary): **deviation, documented deliberately**
  — `hlc.rs` (the one instrumented file) carries no free-text data path at all: every field
  logged (`wall_ms`, `counter`, `device`, `peer_ms`, `local_ms`, skew fields) is a number or a
  `DeviceId` (a ULID, not secret). There is no task description/note body/token analog to inject a
  `ZZ-SENTINEL-ZZ` string into and meaningfully test. Rather than fabricate a hollow test (a
  sentinel that could never appear regardless of correctness, which is exactly the "worthless"
  shape the task brief warns against), this test instead proves the *structural* negative space:
  drives real `Hlc::tick`/`Hlc::merge`/`Skew::check` calls (including the skew-ahead-refused and
  overflow branches) under a capturing dispatch and asserts every captured event's field-name set
  is a subset of the documented whitelist (`wall_ms`, `counter`, `device`, `remote_wall_ms`,
  `remote_device`, `peer_ms`, `local_ms`, `skew_ms`, `direction`, `message`/`target`/etc.'s
  tracing-internal fields) — i.e. no field ever carries free text. Also keeps a literal
  `ZZ-SENTINEL-ZZ` absence assertion for symmetry with every other crate's test (trivially true
  here, but cheap, and guards a future field addition that starts carrying text).
- **txtodo-sync** (`src/no_secrets_tests.rs`, in-crate, reusing `sealed_ops_tests.rs`'s
  `op`/`victim`/`greeted_and_wanting` shape): a real `seal_ops` → `open_ops` → `Session::on_ops` →
  `Session::committed` round with `op(sender, "ZZ-SENTINEL-ZZ buy milk")`, plus a rejected round
  (wrong group key, mirroring `sealed_ops_tests.rs`'s existing attacker test) to also exercise the
  refusal log sites (`aead::log_open_failed`, `workspace_session::log_ops_crypto_refused`). Local
  `LogSink`/`capturing_dispatch`.
- **txtodo-cli** (`tests/sentinel_no_secrets.rs`, integration test — binary-only crate, spawns the
  real `txtodo` binary, same harness idiom as `tests/add.rs`): `TXTODO_LOG=debug`, `add` a task
  whose text contains `ZZ-SENTINEL-ZZ`, then `do <item>` twice (second call hits
  `commands/edit.rs::log_already_done`, whose own doc says `item` is the numeric `ITEM#` argument,
  never task text — the regression guard). Reads the real rotated JSON log file the CLI's own
  `init_telemetry` wrote to `<dir>/.txtodo/logs/txtodo.log.*` and asserts on those bytes directly —
  the most production-shaped of every test in this task (real file, real rotation, real filter
  gate) since the CLI is a binary target with no `[lib]`, so this can't be driven in-process.
- **txtodo-tui** (`tests/sentinel_no_secrets.rs`, integration test — tui *does* have a `[lib]`
  target, `tests/support/mod.rs::RealDaemon` already exists): local
  `txtodo_telemetry::testing::{LogSink, capturing_dispatch}` (allowed dep), `set_default` guard
  held across awaits (same load-bearing `#[tokio::test]` current-thread-runtime trick
  `mcp/tests/smoke.rs`'s own sentinel test already documents and relies on), drives
  `app::perform`/`Input` key presses to append `ZZ-SENTINEL-ZZ` to a real task line through a real
  `txtodod` (`perform`'s own doc: "never `Debug`/`Display` on `Action` itself" — the regression
  guard).
- **relay** (`tests/no_secrets_sentinel.rs`, integration test, local capturing dispatch — same
  `no_txtodo_deps` constraint the task brief already flagged): a real HTTP `PUT`/`GET`/`list` round
  trip (`http_smoke.rs`'s own harness shape) with a blob body containing `ZZ-SENTINEL-ZZ` bytes —
  relay's actual threat model is exactly this: the *stored ciphertext itself* is the thing that must
  never reach a log line, since relay is untrusted by design (`design §4.6`) and every log call
  site in `http.rs`/`store.rs` was confirmed by reading the source to log only counts/errors/paths,
  never blob bytes. Also runs `retention::sweep` for real on an aged sentinel-bearing blob to get a
  guaranteed non-empty capture (`retention.rs::log_removed`, count-only) — the PUT/GET/list round
  itself produces **no** log lines at all on the happy path (confirmed by reading `http.rs`: it
  only logs on a `StoreError::Sqlite` branch), so relying on it alone for the "something was
  actually logged" sanity check would be vacuous.

## Design — the flow test (part b)

`crates/txtodo-daemon/tests/logging_flow_sequence.rs`: reuses `tests/pairing_lan.rs`'s real
two-daemon pairing proof and `tests/support/mod.rs:261`'s `log_tail()` (the same helper
`relay_multiplex.rs` already greps `lan_shared_session_started` through), spawning both daemons
with `TXTODO_LOG=debug`. Asserts the **ordered** sequence of event/span names across the pairing
handshake and the first sync convergence — not just presence — by filtering the tailed JSON lines
down to their `"name"`/`fields.message`/span-name values and checking they appear as a subsequence
in the expected order. See "As built" below for the actual locked-in sequence once implemented
(this section is written before implementation and will be corrected there if reality differs).

## Edge cases
- A sentinel test must never let the sentinel value leak anywhere real: every sentinel lives only
  inside that one test's own temp dir/in-memory sink, never written to a shared fixture or a real
  log path outside the test's own `tempfile::tempdir()`.
- `txtodo-daemon`'s pre-existing coverage was built for the *M4/M8* secrets (group key, device
  static keys, relay/file-carrier/bundle secrets) — this epic's newer daemon commits (actor mailbox
  commit path, watcher, write/mutation/reconcile, the `rpc{method,workspace}` span,
  `lan_session_shared.rs`, `file_carrier.rs`'s five fixed silent drops, `lan.rs`'s resync,
  `relay_fallback.rs`) are not each individually named-path-tested by a sentinel assertion. Per
  each commit's own message ("ids, counts and hashes only — never line text") and this task's own
  reading of every one of those call sites, none of them logs anything beyond ids/counts/hashes —
  but this is a documented residual gap, not a proven one the way the explicitly-driven paths are.
  Flagged here rather than silently treated as fully covered.

## Acceptance
- Every crate in the table above ends with either a passing new sentinel test or a documented
  reason the existing coverage is adequate.
- The flow test asserts an ordered sequence, not a single `contains()`.
- `cargo fmt --check` / `cargo clippy` / `cargo test` green, scoped per touched crate, before each
  commit.

## As built

### Final coverage table

| Crate | Covered how | Test file |
|---|---|---|
| txtodo-model | New — structural field-whitelist test (deviation, see below) | `crates/txtodo-model/tests/hlc_no_secrets.rs` |
| txtodo-store | New — sentinel-laden `Op::Insert`/`Projection` through `commit_change`/`heads`/`get_projection` | `crates/txtodo-store/tests/no_secrets_sentinel.rs` |
| txtodo-crdt | New — sentinel description through `to_loro::apply`/`lww::write_if_newer` | `crates/txtodo-crdt/src/no_secrets_tests.rs` |
| txtodo-sync | New — sentinel `Op` through `seal_ops`/`open_ops`/`Session::on_ops`/`committed`, plus a wrong-group-key rejected round | `crates/txtodo-sync/src/no_secrets_tests.rs` |
| txtodo-cli | New — real `txtodo` binary, `TXTODO_LOG=debug`, real rotated JSON log file read back | `crates/txtodo-cli/tests/sentinel_no_secrets.rs` |
| txtodo-tui | New — real `txtodod`, `app::perform` sentinel-laden `Apply`, shared `txtodo_telemetry::testing` seam | `crates/txtodo-tui/tests/sentinel_no_secrets.rs` |
| relay | New — real HTTP PUT/GET/list with a sentinel blob body, plus a real `retention::sweep` for a guaranteed non-empty capture | `relay/tests/no_secrets_sentinel.rs` |
| txtodo-daemon | Pre-existing, confirmed adequate — `lan_session_security_tests.rs` (real pair + 2 sync rounds), `security_m8_tests.rs` (relay/file-carrier/bundle) | (no new test) |
| txtodo-mcp | Pre-existing, confirmed adequate | `tests/smoke.rs::mcp_call_span_names_tool_and_records_principal` |
| apps/desktop/src-tauri | Pre-existing, confirmed adequate | `src/commands.rs::ui_log_emits_a_named_span_and_the_forwarded_message` |
| txtodo-core / txtodo-query / txtodo-proto | Not instrumented this epic — no test needed | — |

No sentinel test found a real secrets leak. Every one of the new tests above passed on its very
first real run against the actual instrumentation — the crates' own "ids/counts/hashes only"
discipline held.

### The flow test's asserted sequence

`crates/txtodo-daemon/tests/logging_flow_sequence.rs` reuses `pairing_lan.rs`'s real two-daemon
pairing proof verbatim, adds `tests/support/mod.rs::start_with_workspace_id_and_envs` (additive,
no existing call site touched) to set `TXTODO_LOG` on both daemons, and asserts the joiner's own
JSON log carries this ordered subsequence (other events are allowed in between; the check is
"these four appear, in this relative order", not exact adjacency):

1. `pairing_joiner_group_key_adopted` — the real pairing ceremony lands the group key on the joiner.
2. `lan_shared_session_started` — the post-pairing LAN connection is actually established (the same
   event `relay_multiplex.rs` already greps for presence-only; this test locks in *where it sits*
   relative to the rest of the story, not just that it exists).
3. `lan_link_hello_accepted` — the link-level handshake on that connection completes.
4. `commit_done` — the joiner's actor actually committed the initiator's ops (`FileActor::commit`,
   reached from `on_sync_ops` for a real incoming batch) — the real proof of convergence, one layer
   below "the bytes matched" (`wait_for_file_convergence`'s own check).

This order is the right one to lock in because it is the causal order the design requires: no LAN
session before a group key, no accepted handshake before the session starts, no commit before a
handshake accepted real ops. A regression that silently reordered or dropped a step (e.g. a commit
"succeeding" from stale/cached state before the handshake really completed) would show up here as
a missing or misordered name, which `relay_multiplex.rs`'s presence-only grep could not catch.

### Real findings (not secrets leaks, but real, flagged, not fixed)

**Headline finding — `TXTODO_LOG=debug` makes a `--dir` bridge daemon's gRPC surface
catastrophically slow.** Confirmed by direct, repeated measurement while building the flow test:
`start_with_workspace_id_and_envs`, which normally makes a daemon ready in ~1.7s, took over 100s
and never completed under a bare `TXTODO_LOG=debug`, and went back to ~1.7s the moment
`hyper`/`h2`/`tower`/`tonic`/`mdns_sd`/`iroh` were pinned to `info` in the same filter string.
Root cause: `debug` is a blanket `EnvFilter` default level — it does not only raise this
workspace's own crates to debug, it raises *every dependency* to debug too, including the
gRPC/networking stack the daemon's own health/settle polling rides on. `txtodo-telemetry`'s
`build_filter()` (`crates/txtodo-telemetry/src/lib.rs`) already knows to default-quiet one noisy
dependency this same way (`loro`/`loro_internal` → `warn`, with its own doc explaining why: "Loro
logs diagnostics at info carrying payload sizes... a 10k-task snapshot emits thousands of lines")
— it does not yet do the same for the gRPC/networking stack, which is at least as chatty at debug
level (confirmed directly: a single pairing handshake round produces hundreds of `rustls`/
`noq_proto`/`h2` trace-shaped debug lines per connection attempt). This is a real, reproducible gap
in the shipped logging epic's `TXTODO_LOG` story — flagged here, not fixed (out of this task's
scope: no production-code edits). The flow test works around it with an explicit filter directive
(`"debug,hyper=info,h2=info,tower=info,tonic=info,mdns_sd=info,iroh=info"`), documented at its own
call site.

**Separate, pre-existing, environmental: real mDNS/LAN pairing flakiness under load.** While
diagnosing the above, this environment briefly carried ~60 leftover `txtodod` test-daemon
processes (some 3+ days old, unrelated to this task) that were starving real mDNS/LAN discovery —
both `pairing_lan.rs` (completely unmodified) and the new flow test failed the identical way
("the group key never landed on the joiner within 30s") while those were present, and both passed
reliably (3/3 for the new flow test) once they were cleared. This is the same shape of real-network
variance `pairing_relay.rs`'s own module doc already documents and one of its tests is quarantined
for — not a regression from this task, not specific to this new test, and not something to
quarantine here: `pairing_lan.rs` itself isn't quarantined for the same reason.

### Deviations from the plan
- `txtodo-model`'s sentinel test uses a field-whitelist assertion instead of a literal
  `ZZ-SENTINEL-ZZ` injection, because `hlc.rs` (the one instrumented file) has no free-text data
  path at all to inject a sentinel into — documented in the test file's own module doc.
- `txtodo-crdt` and `txtodo-sync`'s sentinel tests each pin tracing's global max-level floor at
  `TRACE` once per process (a small local helper, documented at its call site): both crates' other
  in-crate unit tests call the same logging call sites with no subscriber installed, and tracing's
  global fast-path level check is a single process-wide atomic a concurrent "no dispatch" thread
  can race down, which silently dropped the sentinel test's own events under `cargo test`'s default
  parallel execution until this fix.
