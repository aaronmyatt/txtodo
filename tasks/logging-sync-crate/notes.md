# txtodo-sync: instrument the protocol/pairing/crypto core (root todo.txt line 36)

## Goal

`txtodo-sync` (10,782 lines) has zero logging and no `tracing` dependency — the single biggest
instrumentation gap in the repo. `txtodo-daemon`'s own recent passes (`logging-daemon-boot`,
`logging-daemon-datapath`) made the daemon's *use* of this engine observable, but the engine itself
stays a black box: every `SessionError`/`CryptoError` is a typed `Result` nobody prints, and
`lan_link.rs::recv`'s `IDLE_TIMEOUT` vs a real peer close are the same `LinkError::Closed` at every
call site. This task adds `tracing = "0.1"` (already vetted via the daemon's own `deny.toml` entry)
and instruments exactly the locations the backlog line names: `session.rs:157/177` (the link-level
handshake), `workspace_session.rs:64/83/121/156` (the `Idle -> Greeted -> Wanting -> Importing`
state machine), `pairing.rs:87/131` (offer/accept), `frame.rs` (decode failures), `aead.rs`
(seal/open), and `lan_link.rs:266` (the idle-timeout/close disambiguation).

## Design

### Dependency

`tracing = "0.1"` added to `[dependencies]` only — no subscriber, no init, nothing wired to a sink.
This crate is a library other crates (`txtodo-daemon`) depend on; the daemon already owns the one
`tracing_subscriber` init (`logging-daemon-boot`). `check-boundaries.sh` only gates `txtodo-*` edges
in a manifest, so an external crate like `tracing` trips nothing there; `deny.toml` already allows
its license family via the daemon's own dependency on the same crate, so no new license review.

### Naming errors without leaking payloads: `SessionError::kind()` / `CryptoError::kind()`

Both error enums' `Display` impls already print only ids/counts/states/epochs — never line text,
key material or a signature's bytes (confirmed by reading every arm) — so `%err` would technically
be safe to log. But a `Display` string is a sentence, not a stable event tag a human or a log
aggregator can grep for a specific *kind* of refusal across thousands of lines. So each enum gets an
inherent `pub(crate) fn kind(&self) -> &'static str`, mirroring `workspace_session::name_of`'s
existing precedent for `Message` (one snake_case tag per variant, e.g. `"link_already_greeted"`,
`"wrong_workspace"`, `"decrypt"`). `SessionError::Crypto(_)` maps to the flat tag `"crypto"` in its
own `kind()` — the wrapped `CryptoError`'s own more specific kind is logged directly at whichever
crypto call site produced it (`aead::seal`/`open`, `sign::verify_batch`), not re-derived through the
`SessionError` wrapper, so a session-level log line about a crypto refusal and the crypto module's
own refusal event don't have to agree on how deep to unwrap.

### Wrapper + inner, always

Matching `logging-daemon-datapath`'s own finding (`tasks/logging-daemon-datapath/notes.md`, "As
built"): `#[instrument(skip_all, ...)]` costs real points against `clippy::cognitive_complexity`
(budget 10) on its own, and every additional inline event macro costs more — confirmed here too
while drafting `session.rs::link_hello`. Every instrumented function in this task keeps its public
name and `#[instrument(skip_all, fields(...))]`, delegates its unchanged body to a same-named
`*_inner` sibling with no attribute, and calls a tiny `log_*` helper on the way out. No exceptions,
even where a function looks trivial enough to maybe survive — the daemon task already proved that
intuition wrong often enough not to trust it here.

### `session.rs:157/177` — the link-level handshake

`link_hello`/`on_link_hello` each get `#[instrument(skip_all, fields(device = %self.device, group =
?self.group))]`, wrapping `link_hello_inner`/`on_link_hello_inner` (bodies unchanged) plus
`log_link_hello`/`log_on_link_hello` — success is `debug!` (`"link_hello_sent"` /
`"link_hello_received"`, the latter carrying `skew` since `Skew` is a plain enum, no payload),
failure is `debug!(kind = e.kind(), "link_hello_refused")`. This is the one handshake every
workspace's own `Greet` depends on (`session.rs`'s own module doc), so its refusal reasons
(`GroupMismatch`, `ProtocolMismatch`, `PeerAhead`, `LinkNotReady`, `LinkAlreadyGreeted`, `NotAHello`)
are the first six of the 14 `SessionError` variants this task makes visible.

### `workspace_session.rs:64/83/121/156` — the per-workspace state machine

`hello`, `on_hello`, `on_ops`, `committed` each get `#[instrument(skip_all, fields(from =
?self.state))]` (the state *before* the call — the only cheap, always-known field at entry) wrapping
an `_inner` sibling. Two small log helpers cover all four: `log_state_result` for the three that
return `Message` (logs the new `self.state` read back after the call on success, or `kind =
e.kind()` on failure) and `log_ops_result` for `on_ops` (returns `Vec<Op>`, logs an op count on
success instead of a state — `on_ops` always ends in `Importing` on success, so the state itself
adds nothing `from` didn't already say). `on_ops`'s crypto failure (`verify_batch` refusing a bad
signature or unrecognised device) is the one path in this file logged at `warn!` instead of `debug!`
— every other refusal here (a message arriving in the wrong state, an unrequested range, a
workspace-id mismatch) is routine, validated-not-asserted traffic; a crypto refusal is the specific
"someone sent an op we cannot attribute" case the backlog calls out as a "crypto refusal" in its own
words, and is the one outcome worth a human's attention at a glance. This combination of `from`
(entry state), `kind()` (failure tag) and the state-machine's own four call sites covers the
remaining `Unexpected`, `WorkspaceMismatch`, `Unrequested`, `NotInBatch`, `Gap` and `Crypto`
variants — ten of the 14 total once combined with `session.rs`'s six.

**Deliberately not instrumented**: `session.rs::open_workspace` (`TooManyWorkspaces`) and
`session.rs::workspace`/`workspace_mut` (`UnknownWorkspace`) — the remaining two `SessionError`
variants. The backlog line names four exact locations in `workspace_session.rs` and two in
`session.rs`; these two accessors are not among them, and `session.rs`'s `hello`/`on_hello`/`on_ops`/
`committed` wrappers (which are where `UnknownWorkspace` actually surfaces to a caller) are thin
one-line delegations to the identically-named `workspace_session.rs` methods already instrumented
above — instrumenting both layers would double-log the same call. Flagged rather than silently
dropped; a follow-up task can add a `log_*` call directly in `open_workspace`/`workspace_mut` if a
human wants those two variants surfaced too (both are already `Display`-safe and typed, just not
yet logged).

### `pairing.rs:87/131` — offer/accept

Same wrapper + inner + log_* shape. `offer` gets `fields(device = %own_device, group = ?group)`;
`accept` gets `fields(device = %own_device, peer = %offer.device)`. Both funnel through one shared
`log_pairing_result(op: &'static str, err: Option<PairingError>)` (`"offer"`/`"accept"` as the `op`
field, `PairingError` is `Copy` so no borrow to thread through) — `debug!(op, "pairing_step_ok")` on
success, `warn!(op, error = %e, "pairing_step_failed")` on failure. `PairingError`'s `Display` is
confirmed payload-free (its `Nonce`/`Derive` arms wrap other already-audited error types,
`Seal`/`Closed`/`NotHandshaken`/`NotConfirmed` are static strings), so `%e` is safe here unlike the
raw-`Display` question in `SessionError` above; `warn!`, not `debug!`, because *every* pairing
failure is worth a human's attention (a replayed/expired offer, a rejected SAS, a sealing failure)
— pairing is a rare, human-paced ceremony, not routine traffic the way session state transitions
are.

**File-length budget**: `pairing.rs` was 370/400 lines before this task, only 30 lines of headroom
for two wrapper+inner splits (landed at 416/400 first pass). Paid for by tightening prose across
this file's own doc comments (module doc, `PairingSession`'s struct doc, `Debug`'s doc,
`MAX_FAILED_SAS_CONFIRMATIONS`, `complete`/`sas_words`/`reject`/`wrap_group_key`/
`unwrap_group_key`/`wrap_grant`/`group`, and the two new wrapper docs themselves) down to exactly
400/400 — every fact kept, fewer words each, the same move `logging-daemon-datapath` made for
`actor.rs`/`state.rs`/`global_service.rs` when they hit the same wall.

### `frame.rs` — decode failures

`Frame::decode` gets `#[instrument(skip_all)]` wrapping `decode_inner`, plus `log_decode` logging
`debug!(kind, ...)` on every `FrameError` (`bad_magic`/`truncated`/`too_large`/`unknown_version`, the
four arms already named by the type) and a `trace!` on success (bytes consumed) — `trace`, not
`debug`, because a live link calls this once per inbound chunk read, the same hot-path reasoning
`logging-daemon-datapath` used for `state.rs::apply`. `Frame::peek` is left uninstrumented: it is
`decode`'s own first step (`decode` calls `peek` then re-checks version), so a `peek` failure is
already a `decode` failure the wrapper above reports; a second span one call deeper adds nothing new
and risks double-counting the same truncated-header event twice per read.

### `aead.rs` — seal/open

`seal`/`open` each get `#[instrument(skip_all, fields(group = ?for_.group / group, workspace =
%for_.workspace / workspace))]` wrapping `seal_inner`/`open_inner`, logging `debug!("seal_ok", bytes
= out.len())` / `debug!("open_ok", bytes = plaintext.len())` on success — **byte counts only, never
plaintext or ciphertext** — and `warn!(kind = e.kind(), "seal_failed"/"open_failed")` on error.
`open`'s failure path is the one most likely to see real traffic (a foreign group, a stale epoch, a
tampered tag) so it is the second `warn!`-level site in this task, matching `pairing.rs`'s reasoning:
a batch that fails to open is either a bug or an adversary, never routine.

### `lan_link.rs:266` — `IDLE_TIMEOUT` vs a real peer close

The actual behavior gap the backlog line calls out by name: `IrohLink::recv`'s loop has two
`Err(LinkError::Closed)` returns — `Ok(Ok(None))` (the stream returned EOF: the peer really closed
it) and `Err(_elapsed)` (the `tokio::time::timeout` around the read fired: `IDLE_TIMEOUT`, 750 ms of
silence, no close at all). Both stay `LinkError::Closed` at the type level — the backlog line does
not ask for a wire/API change, and `Link`'s trait signature is `crate`-public API `txtodo-daemon`
already matches against; changing the error shape here is a larger, unscoped break. Instead each
return site gets its own `tracing::debug!` immediately before it: `debug!("link_idle_timeout",
millis = IDLE_TIMEOUT.as_millis())` for the timeout branch, `debug!("link_peer_closed")` for the
real EOF. A caller (or a human reading the JSON log) can now tell the two apart for the first time,
without any change to `Link`'s return type — the disambiguation this task asks for lives in the log,
not the type. `recv` itself is `#[instrument(skip_all)]`-wrapped the same wrapper+inner way as
everywhere else in this task (`recv_inner` holds the loop); this is the one function in this task
expected to run in a hot per-frame loop, so its own per-chunk read is left at whatever level the two
terminal `debug!` calls above already sit at — no `trace!`-level per-chunk event was added, since
`Frame::decode`'s own `trace!` above already covers "a chunk arrived and did/didn't complete a
frame" for the same stream.

## Multi-commit split (300-line diff budget; backlog itself expects 2-3 sessions)

1. `Cargo.toml` (`tracing` dependency) + `session_error.rs` (`kind()`, needed by `session.rs` and
   `workspace_session.rs`) + `session.rs` (`link_hello`, `on_link_hello`).
2. `workspace_session.rs` (`hello`, `on_hello`, `on_ops`, `committed`) + `crypto_error.rs` (`kind()`
   — moved up from the original plan: `on_ops`'s crypto-refusal `warn!` needs it a commit earlier
   than expected, since it names the wrapped `CryptoError`'s own kind directly, not the flat
   `"crypto"` tag `SessionError::kind()` uses).
3. `pairing.rs` (`offer`, `accept`).
4. `frame.rs` (`decode`) + `aead.rs` (`seal`, `open`), reusing `CryptoError::kind()` from commit 2.
5. `lan_link.rs` (`recv`, the `IDLE_TIMEOUT`/peer-close disambiguation).

Each commit independently green: `cargo fmt -p txtodo-sync -- --check`, `cargo clippy -p txtodo-sync
--all-targets -- -D warnings`, `cargo test -p txtodo-sync`.

## Edge cases

- Never log payload bytes, key material or line text — every event above carries only ids, counts,
  hashes, states and the `kind()` tag. `seal`/`open` log byte *counts*, never the bytes themselves.
- `SessionError::Crypto`'s inner `CryptoError` is deliberately *not* unwrapped to its own `kind()` at
  the session-error call sites (see above) — logged as the flat `"crypto"` tag there, with the
  specific `CryptoError::kind()` visible at the crypto module's own call sites instead.
- `lan_link.rs`'s two `Closed` sites keep the same `LinkError::Closed` return type; the
  disambiguation is log-only, not a wire/API change — flagged as a deliberate, in-scope reading of
  the backlog line ("indistinguishable at every call site" is fixed by making the *log* distinguish
  them, since nothing in the backlog line asks for a breaking type change to `Link`/`LinkError`).
- `TooManyWorkspaces`/`UnknownWorkspace` (`session.rs::open_workspace`/`workspace`/`workspace_mut`)
  are out of this task's named scope (see above) — both variants already exist and are `Display`-
  safe; only their own logging call is missing.

## Acceptance

- `cargo fmt -p txtodo-sync -- --check`: clean, every commit.
- `cargo clippy -p txtodo-sync --all-targets -- -D warnings`: clean, every commit.
- `cargo test -p txtodo-sync`: green, every commit.
- Root todo.txt's `logging-sync-crate` line marked done only if every location above landed.

## As built (2026-09-16, agent)

Built exactly to the design above, five commits, one per subtask line:

1. `d88ad9a` — `Cargo.toml` (`tracing = "0.1"`), `session_error.rs` (`SessionError::kind()`),
   `session.rs` (`link_hello`/`on_link_hello`, wrapper+inner).
2. `e24760d` — `workspace_session.rs` (`hello`/`on_hello`/`on_ops`/`committed`, wrapper+inner) +
   `crypto_error.rs` (`CryptoError::kind()`, moved up from the original plan — see below).
3. `20a0fb3` — `pairing.rs` (`offer`/`accept`, wrapper+inner; file-length prose trim to 400/400).
4. `89de848` — `frame.rs` (`FrameError::kind()`, `Frame::decode`) + `aead.rs` (`seal`/`open`).
5. (this commit) — `lan_link.rs` (`IrohLink::recv`, the `IDLE_TIMEOUT`/peer-close disambiguation).

### The recurring wrinkle, sharper than the daemon's own version

`logging-daemon-datapath` found that `#[instrument]` alone can cost real `cognitive_complexity`
points. This task found something one level worse: **a bare `tracing::debug!`/`warn!`/`trace!`
call nested inside a `match` arm or an `if` branch costs real points on its own, independent of
`#[instrument]` entirely** — confirmed the moment `session.rs::log_link_hello` (a *plain*,
un-instrumented free function, just a 2-arm match each calling one `tracing::debug!`) measured
16/10. Every `log_*` helper in this task therefore follows a stricter rule than the daemon's own
precedent: **no `tracing` macro call may appear directly inside a `match` arm or `if` branch of a
function that also contains other branches.** Two techniques cover every case found:

- **Branch-free fields** (`session.rs::log_link_hello`, `log_on_link_hello`,
  `workspace_session.rs::log_state_result`): when the only thing that varies between `Ok`/`Err` is
  which optional fields are present, skip the `match` entirely — `r.as_ref().err().map(F::kind)`
  gives `Option<&'static str>`, and `tracing`'s own `Value` impl for `Option<T: Value>` records
  nothing when `None`. Combinators (`.map`/`.as_ref`) are not control-flow keywords, so they add
  nothing to `cognitive_complexity` the way `if`/`match` do.
- **One-macro-call leaf functions** (`workspace_session.rs::log_ops_ok/_crypto_refused/_refused`,
  `frame.rs::log_decode_ok/_failed`, `aead.rs::log_seal_ok/_failed`/`log_open_ok/_failed`,
  `pairing.rs::log_pairing_ok/_failed`, `lan_link.rs::log_link_peer_closed/_idle_timeout`): needed
  whenever the branches must pick a different **macro** (`debug!` vs `warn!`), which no field trick
  can express. Each arm calls a dedicated function containing *only* one `tracing` call and nothing
  else; the dispatching `match`/`if` itself holds no macro call, so its own complexity stays low
  regardless of how many arms it has. `lan_link.rs::recv_inner` takes this furthest: each leaf
  function returns `LinkError::Closed` itself (`return Err(log_link_peer_closed())`), so the
  already-branchy 4-arm-inside-a-loop function gains a plain function call at each site, never a
  macro invocation directly in its own body.

### `CryptoError::kind()` landed one commit earlier than planned

The original plan (notes above, first draft) put `CryptoError::kind()` in commit 4 alongside
`frame.rs`/`aead.rs`, on the theory that `SessionError::Crypto` would log the flat `"crypto"` tag
and nothing needed the real `CryptoError::kind()` before then. Building `workspace_session.rs`'s
`on_ops` crypto-refusal `warn!` event found that untrue: that event *does* want the wrapped
`CryptoError`'s own specific kind (`"signature_invalid"`, `"unknown_device"`, etc.), not the flat
tag — a session-level "some crypto thing failed" is far less actionable than knowing which. So
`CryptoError::kind()` moved to commit 2, used immediately by `on_ops`'s own `log_ops_crypto_refused`
and reused unchanged by `frame.rs`/`aead.rs` in commit 4.

### `pairing.rs`'s file-length budget: landed at 416/400 first pass, trimmed to exactly 400/400

As anticipated in the design: two wrapper+inner splits added ~45 lines to a file with only 30 of
headroom. Paid for by tightening prose across the file's own existing doc comments (module doc,
`PairingSession`'s struct doc, `Debug`'s doc, `MAX_FAILED_SAS_CONFIRMATIONS`, `complete`/
`sas_words`/`reject`/`wrap_group_key`/`unwrap_group_key`/`wrap_grant`/`group`, and the two new
wrapper docs themselves) — every fact kept, fewer words each — landing at exactly 400/400, the same
move `logging-daemon-datapath` made for `actor.rs`/`state.rs`/`global_service.rs`.

### Verification

- `cargo fmt -p txtodo-sync -- --check`: clean, every commit.
- `cargo clippy -p txtodo-sync --all-targets -- -D warnings`: clean, every commit — no
  `#[allow]`/`#[expect]` added anywhere; every complexity hit was fixed by restructuring, not
  suppressed.
- `cargo test -p txtodo-sync` (182 tests, all `*_tests.rs` modules compiled into the lib target,
  this crate has no separate `tests/` integration dir): green after every commit, no new failures,
  3 pre-existing `#[ignore]`d tests untouched (the same-process `iroh` connect bug, unrelated to
  this task).
- `.claude/scripts/check-file-length.sh` and `check-boundaries.sh`: clean after every commit —
  `tracing` is an external crate, so `check-boundaries.sh` (which only gates `txtodo-*` edges)
  never had anything to say about it, exactly as the orchestrating brief predicted.
- Manual proof events actually fire and stay payload-free was not captured via
  `txtodo_telemetry::testing` (that crate is intentionally out of reach for this crate — see the
  brief's own "Context you need"); instead, `session_tests.rs`/`workspace_session`'s own existing
  test suite exercises every instrumented path (`link_hello_checks_group_protocol_and_clock_before_
  anything_wants_anything`, `every_message_out_of_order_is_refused_and_changes_nothing`, the two
  multiplex tests, `sealed_ops_tests`, `pairing_tests`, `frame_tests`, `aead_tests`) and all 182
  stayed green with the instrumentation compiled in — proof the new spans/events don't change any
  observable behavior, though no subscriber was installed in-process to capture and assert on the
  JSON lines themselves (this crate deliberately never installs one; that is the daemon's job).

### Deliberately out of scope

- `session.rs::open_workspace`/`workspace`/`workspace_mut` (`TooManyWorkspaces`/`UnknownWorkspace`)
  — not named by the backlog line's four `workspace_session.rs` + two `session.rs` locations; see
  "Deliberately not instrumented" above.
- No `#[instrument]` on `Frame::peek` (subsumed by `decode`'s own span/event) or on `IrohLink::send`
  (the backlog line names only `lan_link.rs:266`, `recv`'s own `Closed` ambiguity — `send`'s error
  path has no equivalent ambiguity to fix and adding a span there was never asked for).
- No change to any `txtodo-daemon` file, any other `+m11 @observability` line, or any file outside
  `crates/txtodo-sync/`, `tasks/logging-sync-crate/` and this one root todo.txt line — held to
  exactly the scope the orchestrating brief set.
