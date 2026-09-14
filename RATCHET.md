# Ratchet board — txtodo

Human-facing debt board. Seeded by `/setup` on 2026-09-11; append-only for agent writes from here on
(the fence asks on anything that is not a pure append). `/ratchet` burns one item down per session.
Measured counts live in the tool baselines and are recomputed, never stored here as truth.

## Baselined at setup

- Greenfield: 0 lint violations, 0 type errors, 0 files over 400 lines. `budgets.json.baselinePaths` is empty.
- Coverage measured 0% (three empty `fn main` lines, zero tests). Floor set to the measured number, per the skill rule.

## Priorities

1. **Coverage floor 0 → 80.** Raise `budgets.json.coveragePercent` once `txtodo-core` (plan M1) lands its corpus and property tests. First `/ratchet` promotion.
2. **Gate counts renames as delete + add.** `gate.sh` and `guardrails/index.ts` use `git diff HEAD --numstat`; a pure rename of a 500-line file costs 1000 lines. Switch both to `-M` (rename detection) and count untracked files after `git add -N`. Seen live on 2026-09-11 when the design docs were renamed.
3. **Windows CI skips the three bash scripts** (boundaries, file length, assertions). Port to a `cargo xtask` or accept Linux/macOS-only for tier-3 checks and say so in `stack.md`.
4. **Perf budgets are tier 4 until M3.** Plan §5 numbers (parse 100k lines ≤ 150 ms, reconcile ≤ 20 ms) become criterion benches in CI when the code exists.

## Campaigns

_(none in flight)_

## Ejected

_(none)_

## Exceptions granted

_(none)_

---
2026-09-11 · Campaign: coverage floor 0 → 80 (priority 1). Measured after M1 core tests: workspace 97.07%
line coverage (txtodo-core 88–100% per file; the empty crates' `fn main` are the misses). Set
`budgets.json.coveragePercent = 80`, `commands.testCoverage --fail-under-lines 80`, justfile, CLAUDE.md row,
stack.md rows. Priority 1 closed.

---
2026-09-12 · Exceptions granted (loro CRDT engine, plan M4 `crdt-loro-doc`). Supersedes the
"_(none)_" placeholder in the Exceptions section above. `deny.toml` changes, narrowest scope:

- **RUSTSEC-2026-0247 (bitmaps), RUSTSEC-2026-0248 (im), RUSTSEC-2026-0251 (sized-chunks)** —
  unmaintained, transitive via `loro-internal → im`. Repos archived 2026-05-03; no safe upgrade; no
  known vulnerability. `advisories.ignore` for these three IDs only.
- **RUSTSEC-2023-0089 (atomic-polyfill)** — pre-existing via `postcard → heapless 0.7 → txtodo-model`,
  NOT introduced by loro: `cargo deny check advisories` fails on the loro-free tree too. Ignored to
  keep deny green; revisit when postcard drops heapless 0.7.
- **BSL-1.0 license (xxhash-rust)** — Boost Software License, OSI-approved and permissive; added to
  `licenses.allow`. Transitive via `loro-internal`, `loro-kv-store`.

No blanket `unmaintained = allow` was taken; the four IDs are named individually.

---
2026-09-13 · Board review after the last 100 commits (M4 close-out, M5, M6 foundation, M11 planned).
Measured today, not stored as truth: 606 `#[test]`s; 6 `#[ignore]`s; txtodo-daemon 77 src files,
~14.1k lines, 38 deps; 10 of the last 100 commits are "gate fix: split X for complexity".

Priorities, re-scored:

- **2 (gate counts renames as delete + add): still open.** `.claude/hooks/gate.sh:17` is still
  `git diff HEAD --numstat` with no `-M`. Unchanged since setup.
- **3 (Windows CI skips the bash scripts): still open and wider.** `ci.yml` "lint (windows)" and
  "test (windows)" now also `--exclude txtodo-cli --exclude txtodo-daemon`. ADR 0007 promises a
  Windows desktop; Windows CI proves core, model, store, crdt, sync only. Either say so in
  `stack.md` or make the named-pipe transport (desktop `daemon.rs:51` "Windows transport lands
  M10") its own todo line.
- **4 (perf budgets tier 4 until M3): closed for parse and reconcile** — `check-bench.sh` against
  `budgets.perf`. `sync-bench-m4` (todo line 11) waits on the two-daemon fixture
  (ABSTRACTIONS.md 2026-09-13, last entry).

New debt, in the order I'd burn it:

5. **The second stack is ungated.** `apps/desktop`: 101 files touched in the last 100 commits,
   ~5k lines of Svelte/TS/Rust. `ci.yml` has no node step; `check-file-length.sh:21` walks
   `crates/` only; `stack.md` still says "no framework bends a rule at M0, revisit at M7". M7 is
   here; todo line 34 (`desktop-stack-mapping`) is open. `Devices.svelte` is 333 lines,
   `FileView.svelte` 307 — over the Rust ceiling minus margin, unmeasured. Campaign proposal: a
   `desktop` CI job (`npm ci`, `npm run check`, `npm test`, `cargo clippy` in `src-tauri`), extend
   `check-file-length.sh` to `apps/**/*.{svelte,ts,rs}`, write the `stack.md` section.
6. **File ceiling reached in the daemon.** `state.rs` and `reconcile_sidecar.rs` sit at exactly
   400; `tests/grpc.rs` 394; `txtodo-crdt/src/doc.rs` 392. M11's `WorkspaceActor` (todo line 132)
   adds a layer on top. Proposal: split the *crate*, not more files — `txtodo-daemon-notes`
   (the eight `notes_*.rs`), `txtodo-daemon-refdir` — which needs new `budgets.slices.allowedDeps`
   rows, a human-owned edit.
7. **Complexity-split churn.** Ten of the last 100 commits exist only to satisfy
   `cognitive-complexity 10` / `fn 60` (`commit`, `on_external_change`, `load_mirror`,
   `state_after`, `review_rows`, `flush`/`converge_mirror`). Not a loosening request (the agent
   never proposes one). Recorded so a reviewer checks those splits are cohesive units, not
   `commit_part_2`.
8. **todo.txt drifts from shipped code, and the project does not dogfood its own tool.**
   - Ten `@desktop` lines (25–33) are open with no `ref:` while 583af82, c1160ce, 77e4df3, 23243ca,
     a243fb7, cb682b7 shipped the main view, edit popover, conflict banner + review sheet, devices
     screen and Lezer grammar.
   - Line 2 (`sync-lan-transport`) says "the only remaining gap is the upstream iroh bug", but
     `tasks/sync-lan-transport/notes.md` pass 3 says wiring `Discovery`/`Link` into `txtodod` was
     "not attempted" — nothing in `crates/txtodo-daemon/src` references either. That wiring is
     unblocked work hiding behind a blocked-sounding line.
   - Status is appended to descriptions as prose (`— blocked: …`), which the project's own parser
     reads as description text. Appendix A extension tags exist for this (`status:`).
   - `done.txt` is 0 lines; ~90 `x` lines have never been archived. `txtodo archive` on the
     repo's own list is a one-command dogfood.
9. **Ignored tests carrying real blockers.** `txtodo-sync/src/endpoint_tests.rs:74` (noq-proto
   1.3.0 refuses `127.0.0.1` paths — the whole M4 LAN blocker), `txtodo-crdt/tests/conflicts.rs:321`,
   `tests/sim.rs:351` (deliberate, `just sim`). Re-check trigger for the first: any `iroh` or
   `noq-proto` bump in `Cargo.lock`.

Exceptions granted (recorded late — taken 2026-09-12 in `tasks/sync-lan-transport/notes.md`, not on
this board; `deny.toml:15-21` carries the dated comments):

- **Unlicense** (`pharos`, `async_io_stream`, `ws_stream_wasm`) and **CDLA-Permissive-2.0**
  (`webpki-root-certs`, `webpki-roots`) added to `licenses.allow`, transitive via `iroh`'s
  unconditionally compiled relay code. Same narrow, named-crate pattern as the loro entry above.

---
2026-09-13 · Correction to item 8 above: design Appendix A has no `status:` key (`id pri due t rec h
ref` only). Adding one is a design decision, not hygiene, so the `— blocked: …` prose stays as is.
Acted on the rest of item 8 the same day: closed the four shipped `@desktop` lines (main view, edit
popover, conflict review, devices) and the answered Q1/Q3/Q4 lines, archived to `done.txt`, and added
todo lines for Q7–Q11. Detail view, quick-add, raw mode, Playwright, visual regression and the
stack mapping are genuinely unbuilt and stay open; the Lezer line stays open for its missing CI step.

---
2026-09-13 · `security-m4-review`: M4 security checklist, item by item (plan §5's one-line
checklist: "no secrets in logs; keys only in keystore; every network message versioned,
authenticated, encrypted; MCP HTTP refuses non-loopback unless `--lan`; tokens never logged; path
traversal impossible via `ref:` (fuzz the slug validator); relay cannot distinguish op types.").
Full task in `tasks/security-m4-review/`.

- **No secrets in logs — pass.**
  `crates/txtodo-daemon/src/lan_session_security_tests.rs::no_secrets_appear_in_logs_across_a_real_pair_and_sync`:
  a real pairing round trip (mints/wraps the group key and each side's device static key) plus two
  real LAN sync rounds (one happy, one a peer sealing under a key nobody holds, so
  `lan_session.rs`'s own `tracing::debug!`/`warn!` sites actually fire) captured through a scoped
  subscriber shaped like `telemetry.rs`'s production JSON layer; neither the group key nor either
  device's static key ever appears in the captured text, hex or raw.
- **Keys only in keystore — pass.** `crates/txtodo-daemon/tests/security_keys_only_in_keystore.rs`,
  two tests: no `env::var`/`env::var_os`/`.var()` call anywhere in `crates/`/`apps/` reads a name
  suggesting key material, and no field on the actual argv/config surface (`txtodod`'s and
  `txtodo`'s CLI arg structs, both configs) is named or typed like it could carry raw key bytes.
  Scoped deliberately to that surface rather than a whole-workspace field scan — the test's own doc
  names the three false positives (a SQLite meta row `key`, a `key_epoch` rotation counter, the
  keystore's own legitimate internal `passphrase`/`Secret` fields) a broader scan produced first.
- **Every network message versioned, authenticated, encrypted, including `Hello` — pass.**
  `crates/txtodo-sync/src/hello_wire_tests.rs`, six tests. `Hello` is not a special case in this
  codebase: `txtodo-daemon`'s one production send path (`lan_session.rs::send_message`) seals every
  `Message` whole under the group key before it reaches a `Link`, so `Hello` is never sent in the
  clear and — contrary to the concern in `tasks/security-m4-review/notes.md` — never before a
  shared key exists at all (`drive_session` fetches the group key first and does nothing if there
  isn't one). Confirmed: a sealed `Hello` never happens to decode as plaintext; it cannot be opened
  with the wrong group, wrong key, or no key; a captured `Hello` replayed at the same session once
  past `Greeted` is refused outright and changes no state; replayed to a fresh session it yields
  only the same public `Want` a legitimate peer would get and never advances heads; a tampered or
  foreign-protocol sealed `Hello` fails the same way `sealed_ops_tests.rs` already proves for `Ops`.
  - **Open, `@human`-tagged, not closed by this pass** (`tasks/security-m4-review/todo.txt` line 4):
    `Hello.heads` is still a privacy leak *within* the group — any paired device learns how much
    every other device has written, inherent to a diff-based sync protocol, not a bug this session
    can fix. It is never visible outside the group (AEAD-sealed), so the real question is narrower
    than the task notes framed it: "is it acceptable that group members see each other's heads,"
    not "is it acceptable that anyone does." Left for the human to decide and record.
- **Path traversal impossible via `ref:` (fuzz the slug validator) — fail, real gap found.** New
  fuzz target `crates/txtodo-core/fuzz/fuzz_targets/slug_windows_safe.rs`, run `cargo fuzz run
  slug_windows_safe -- -max_total_time=30` (nightly toolchain + `cargo-fuzz` installed for this
  pass; neither is present by default in this sandbox). It crashed on the first seed:
  **`is_valid_slug("con")` is `true`.** `is_valid_slug` (`crates/txtodo-core/src/task.rs:13`)
  already rejects `/`, `\`, `..`, a bare `.`, a leading `.`/`-`/`_`, and anything outside
  ASCII-lowercase/digit/`.`/`_`/`-` — which also makes a NUL byte and a leading dot structurally
  unreachable (both asserted and passing in the new target) — but never checks for a
  Windows-reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`), which
  Windows reserves regardless of any extension (`con.txt` still names the `CON` device, not a file
  called that). `txtodo-core` is a frozen path this session was not asked to edit, so
  `is_valid_slug` is unchanged; this is not classic path traversal (it cannot escape the `ref:`
  directory) but it is a real, fuzzer-confirmed, cross-platform filesystem-safety gap that M5's
  real directory-building work will hit on Windows (ADR 0007 already promises Windows desktop
  support). **Follow-up needed**: extend `is_valid_slug` (or add a second check M5 calls before
  creating a directory) to reject the reserved-name list. Crash artifact (gitignored, reproduced
  above for the record): `crates/txtodo-core/fuzz/artifacts/slug_windows_safe/crash-b1f6e510eb0f015b9d2bd5b22764cd95ae00d908`.
- **MCP HTTP refuses non-loopback unless `--lan` — deferred to M6.** No MCP HTTP surface exists
  before M6 (`txtodo-daemon/CLAUDE.md`'s own `payloadKB` note: "N/A until M6: no HTTP surface in
  M0-M5"). Not buildable yet; noted per the task's own scope, not silently dropped.
- **Tokens never logged — deferred to M6.** The M6 token data layer (`crates/txtodo-daemon/src/tokens.rs`,
  `TokenCreate`/`List`/`Revoke`) landed early (M4 pass, design §6.2), but request-time bearer
  enforcement — the part where a token could plausibly end up in a log line — is explicitly M6's
  larger MCP-auth-server milestone (`crates/txtodo-daemon/CLAUDE.md`'s own invariant). In passing:
  `tokens.rs` has zero `tracing::` calls today, so nothing logs a token now either — observed, not
  asserted by a dedicated test this pass, since the request-time surface that would need one
  doesn't exist yet.
- **Relay cannot distinguish op types — deferred to M8, known gap recorded now.** No relay exists
  yet. The M4-relevant half, recorded so it is not discovered late: every sealed frame's on-wire
  length is visible to anyone forwarding it (no padding scheme exists), and `Hello`/`Want`/`Ack`/`Ops`
  have different, characteristic size distributions — a small few-head `Hello` or an empty `Ack`
  looks nothing like a multi-op `Ops` batch. A future relay that cannot read the group key could
  still often guess message *type* by ciphertext length alone, short of the checklist's actual goal
  even though today's design already keeps the *content* (op payloads, ids, heads) fully opaque.

Follow-ups this pass could not take, since editing the root `todo.txt` was this session's
orchestrating task's job, not this one's:
- `is_valid_slug`'s Windows-reserved-name gap, above (an `@core` or `@sync-ref-slug`-tagged line).
- M6 todo line(s): MCP HTTP loopback-only unless `--lan`; tokens never logged, request-time.
- M8 todo line: relay op-type leakage via frame length, recorded above.
- The `@human` `Hello`-heads privacy decision (`tasks/security-m4-review/todo.txt` line 4).

---
2026-09-14 · `security-m8-review`: M8 security checklist, the three carriers that did not exist at
M4 — relay, file carrier, bundle. Same plan §5 checklist as the M4 pass above. Full task in
`tasks/security-m8-review/`.

- **No secrets in logs — pass.**
  `crates/txtodo-daemon/src/security_m8_tests.rs::no_secrets_appear_in_logs_across_relay_file_carrier_and_bundle`:
  a real relay put/get cycle (`relay::store::Store`, wake-up drained through `relay::push::NoopPush`),
  a real `FileCarrier` send/recv cycle, and a real bundle export/import cycle (real workspace, a
  freshly minted device signing key, a real passphrase) — all under one capturing `tracing::Dispatch`.
  Every cycle moves a real, distinctive plaintext line sealed under a real group key with the actual
  production `seal_ops` path, so a leak would show up as exactly the bytes an attacker would want.
  None of the group key, either device signing key, the bundle passphrase, or the plaintext line
  appear in captured log text. Reuses `lan_session_security_tests.rs`'s M4 `LogSink`/
  `capturing_dispatch`/`captured_text`/`hex` helpers rather than duplicating them.
- **Keys only in keystore — pass.**
  `crates/txtodo-daemon/src/bundle_tests.rs::wrong_passphrase_writes_nothing_and_the_manifest_carries_no_key_material`:
  the group key never leaves the passphrase wrap; the bundle manifest carries no key bytes in the
  clear. M4's own keystore-argv/env static check is unchanged and still green.
- **Every network message versioned, authenticated, encrypted, including `Hello` — pass.**
  Relay: `crates/txtodo-daemon/tests/relay_converge.rs` asserts the relay run moves the same sealed,
  versioned, authenticated frames as LAN (`sync-crypto-envelope`), not a shortcut. File carrier: no
  `Hello`/handshake message exists there at all — it is append-and-poll against a shared folder, not
  a live connection — and it only ever moves the identical sealed `Frame` format, now also proven
  ciphertext-only on disk by
  `crates/txtodo-sync/src/carrier_tests.rs::ops_on_disk_are_ciphertext_only_never_plaintext_or_the_group_key`.
  `Hello` itself was already closed at M4, above; unchanged.
- **Path traversal impossible via `ref:` (fuzz the slug validator) — pass, plus a new real gap found
  and fixed.** M4's `ref:`-slug fuzz target is unchanged. New for M8: `--sync-dir`'s entire threat
  model is an untrusted shared folder (Syncthing/Dropbox/iCloud Drive), and `carrier.rs`'s directory
  scan (`read_dir`, backing `highest_rotation`/`other_device_files`) collected every entry's name
  with no check on entry type — a malicious peer sharing that folder could plant a symlink named like
  a valid other-device `<ulid>[-<n>].ops` file, pointing at an arbitrary local path (e.g.
  `~/.ssh/id_rsa`), and `read_tail`'s `std::fs::File::open` would follow it. Confirmed real by
  regression: with the fix reverted, the new test read a planted sentinel straight through the
  symlink. Fixed: `read_dir` now checks `std::fs::symlink_metadata` (never the symlink-following
  `metadata()`) and skips any symlink before it is treated as a candidate `.ops` file — silently, not
  a hard error, since a symlink there is not necessarily hostile and the directory is rescanned every
  poll. Tests:
  `crates/txtodo-sync/src/carrier_tests.rs::a_symlink_planted_by_a_malicious_peer_in_sync_dir_is_never_followed`
  and `::parse_ops_file_name_rejects_dot_dot_and_slash_bearing_candidates` (the latter proves, does
  not change, that a `..`- or `/`-bearing candidate already fails `Ulid::parse`).
  - **Flagged, not fixed this pass** (out of scope, filed separately): `FileCarrier::write_frame_to`'s
    own-file write path also does not check `symlink_metadata` — a peer could pre-plant a symlink at
    *our own* device's filename before we ever write, corrupting whatever it points to. Read-side
    only was this pass's scope; the write side needs its own task.
- **Relay cannot distinguish op types — split: content pass, frame-length deferred, not
  milestone-scheduled.** Content: `crates/txtodo-daemon/tests/relay_converge.rs::relay_store_holds_only_opaque_ciphertext`
  confirms the relay's own store holds only opaque `Vec<u8>` blobs and its logs carry routing
  metadata only, never payload — the M4-deferred half of this item closes here. Frame length does
  not: every sealed frame's on-wire length is still visible to anyone forwarding it (no padding
  scheme exists), and `Hello`/`Want`/`Ack`/`Ops` still have distinct, characteristic size
  distributions — unchanged since the M4 entry above first recorded it. Deferred again, this time
  with a tracked line rather than only a RATCHET.md paragraph: root `todo.txt`
  id `06G9ZV2JPBJ8P829RA634BE8KG`. Fixing it for real needs a padding scheme, a protocol change, not
  a test — a human design call this pass was not asked to make.
- **MCP HTTP refuses non-loopback unless `--lan` — still deferred to M6.** Unchanged from the M4
  entry: no MCP HTTP surface exists before M6, and M6 itself is still deferred (root `todo.txt`,
  "core app + desktop prioritized first," reaffirmed 2026-09-13). Not buildable yet.
- **Tokens never logged — still deferred to M6.** Unchanged from the M4 entry, same reason: request-
  time bearer enforcement is M6's larger MCP-auth-server milestone, which has not landed. `tokens.rs`
  still has zero `tracing::` calls, so nothing logs a token now either — still observed, not asserted
  by a dedicated test, since the surface that would need one still does not exist.
  - Both M6 deferrals are now tracked from the milestone that owns them, not just here: the existing
    M6 `security checklist review` line in root `todo.txt` (id `01M2B4ZWQD90N6M6Q7HH4JACWP`) was
    amended to name this M8 dependency explicitly, rather than adding a duplicate line.

`security-m8-review`'s parent line closes with this entry — every item above is `pass` or carries
its own tracked `deferred` line; none is a silent gap.
