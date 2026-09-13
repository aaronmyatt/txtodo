# Pairing: `txtodo pair` shows a QR + 6-word SAS from an X25519 handshake, then snapshot + ops — M4

Plan M4: "`txtodo pair` shows a QR + SAS (6 words from the EFF short list) derived from an X25519
handshake; the other device runs `txtodo pair <code>` or scans. Result: both devices hold the group
key; new device receives a full snapshot then ops."

## The SAS must commit to both identities, or it does nothing

A short authentication string exists to catch an active machine-in-the-middle. It only does that if
an attacker running two sessions cannot make both sides show the same words. So the SAS is **not**
derived from the shared secret alone — it is derived from the whole transcript:

```
transcript = protocol_version || device_a || pub_a || device_b || pub_b || group_id
sas_bytes  = HKDF-SHA256(ikm = x25519(priv, peer_pub), salt = transcript, info = b"txtodo-sas-v1")
```

Getting this wrong is the single failure mode of the task, and it is invisible in testing — both
honest devices agree either way. Write the MITM test (below) or the guarantee is decorative.

Binding `protocol_version` into the transcript also kills downgrade: an attacker cannot talk v1 to
one side and v2 to the other without changing the words.

## Words

EFF short list, 1296 words (<https://www.eff.org/dice>, `eff_short_wordlist_1.txt`). Six words is
`6 × log2(1296) ≈ 62 bits` — plenty against an online attacker who gets one try inside the window.

- Vendor the list, checked in, with a test asserting 1296 entries, no duplicates, and a hash of the
  file. A wordlist that silently changes changes every SAS.
- Index by `sas_bytes` chunks; document the exact bit-slicing, because the two implementations that
  must agree are the same code today and a phone app at M9.

## Confirmation must be mutual

Both humans compare, both press yes. A one-sided confirm lets an attacker who controls the display
on one device complete the pairing. The group key is sent **after** both confirmations, encrypted
under a key derived from the same transcript (`info = b"txtodo-pair-v1"`, separate from the SAS
info string — never reuse one derived key for two purposes).

## The QR carries no secrets

`device`, `group_id`, the initiator's X25519 **public** key, the endpoint address, and a one-time
pairing nonce. Nothing in it is confidential; a photographed QR must be useless without the SAS
confirmation on the other end. `txtodo pair <code>` takes the same payload base32-encoded for people
without a camera.

## Bounds

- `PAIRING_WINDOW_MS` (leaning 120 000) after which the offer expires and the nonce is discarded.
- Single use: a nonce is consumed on first handshake, whether it succeeds or fails.
- `MAX_CONCURRENT_PAIRINGS`, asserted — one is the honest number; the cap stops a flood from
  holding ephemeral keys open.
- Rate-limit failed SAS confirmations, then close the window entirely rather than allow retries.

## Snapshot then ops

Once keyed, the new device gets a full snapshot (store `snapshots` table) and then the op tail. This
is the one path where a device legitimately holds no history, so it is also the easiest place to
accidentally accept an unsigned bulk import — verify every op in the snapshot's op range exactly
like a normal batch ([sync-crypto-envelope](../sync-crypto-envelope/notes.md)). Bound the snapshot
size and stream it; do not buffer a whole workspace in memory.

## Tests

- **MITM**: a relay that runs two handshakes and forwards. The two SAS strings must differ. This is
  the acceptance test for the whole task.
- Same transcript on both sides yields identical words; any single-bit change to any transcript
  field changes them.
- An expired nonce, a reused nonce, and a one-sided confirmation each fail with distinct errors and
  transfer no key.
- The QR payload, decoded, contains no key material — assert on the decoded struct's fields, so a
  future field addition has to be considered.
- Wordlist invariants (count, duplicates, file hash).

## As built (2026-09-12, agent) — the `txtodo-sync` crypto/state-machine half only

- `crates/txtodo-sync/src/wordlists/eff_short_wordlist_1.txt`: the real EFF short wordlist (list 1,
  fetched from eff.org, dice-index column stripped), 1296 lines, no duplicates. Loaded via
  `eff_wordlist::wordlist()`; `WORDLIST_SHA256` pins the vendored file's own bytes.
- `crates/txtodo-sync/src/transcript.rs`: `transcript(protocol_version, Party, Party, GroupId)`.
  `Party { device, public_key }` replaces four positional args to stay under this workspace's
  5-argument limit. Order is canonical (ascending `DeviceId`), not initiator/joiner, so either side
  computes the same bytes from its own "self/peer" view without agreeing out of band on roles.
- `crates/txtodo-sync/src/sas.rs`: one `expand()` (HKDF-SHA256 extract-then-expand) feeding both
  `sas_words` (`info = "txtodo-sas-v1"`) and `pair_key` (`info = "txtodo-pair-v1"`), so the two
  outputs are provably from different `info` labels, never the same derived bytes reused. Each SAS
  word's index is a `u32` (not the literal bit-slice the notes sketch) read from its own 4-byte
  chunk of a `SAS_WORD_COUNT * 4`-byte expand, taken mod 1296 — documented bias (`≤ 1296 / 2^32`) in
  the doc comment rather than the more complex rejection-sampling that bias would otherwise call for.
- `crates/txtodo-sync/src/offer.rs`: `PairingOffer` (device, group, ephemeral X25519 public key,
  endpoint hint, nonce, **`issued_at_ms`**) plus postcard/base32 codecs. `issued_at_ms` is not in the
  notes' field list — added because the joiner has no local record of when the initiator issued the
  offer (see the nonce-registry judgement call below) and needs it to enforce `PAIRING_WINDOW_MS`
  independently. Still no secrets: an attacker forging the timestamp can only make an already
  nonce-bound, transcript-bound offer look older or newer, not extend it.
- `crates/txtodo-sync/src/nonce_registry.rs`: `PAIRING_WINDOW_MS` (120 000 ms), `MAX_CONCURRENT_PAIRINGS`
  (1, per the notes' own reading: "one is the honest number"). `NonceRegistry::issue`/`consume` is the
  initiator's own single-use/expiry bookkeeping on a nonce it minted; `witness` is a second method for
  the joiner, which never called `issue` on this nonce and so has no `open` record to check expiry
  against — it checks the offer's own `issued_at_ms` instead. Both maps are pruned (an unexpired-open
  cap plus a consumed-record retention window) so a long-running daemon's tables stay bounded.
- `crates/txtodo-sync/src/pairing.rs`: `PairingSession` — `offer`/`accept`/`complete` run the X25519
  ECDH and compute the transcript+SAS+pair-key eagerly; `confirm_local`/`confirm_remote`/`reject`
  track mutual confirmation; `is_ready_to_send_key` requires both flags and `!closed`;
  `wrap_group_key`/`unwrap_group_key` seal/open under `pair_key` with the transcript as AEAD
  associated data, refusing outside that state (`Closed` takes precedence over `NotConfirmed` when
  both would apply). `MAX_FAILED_SAS_CONFIRMATIONS = 3` closes the window on repeated mismatch,
  per "rate-limit, then close" rather than closing on the first one.
- `cargo deny check` run 2026-09-12: `advisories ok, bans ok, licenses ok, sources ok`.
  `x25519-dalek` 2.0.1, `hkdf` 0.12.4, `sha2` 0.10.9, `data-encoding` 2.11.1, `rand_core` 0.6.4 —
  all MIT/Apache-2.0.
- Judgement calls, flagged for the human:
  - Bit-slicing is 32-bit-chunk-then-modulo, not literal bit-slicing, and the resulting small bias
    is documented rather than eliminated by rejection sampling — flagged in case the human wants the
    stricter version.
  - `PairingOffer` gained `issued_at_ms` (see above); not in the notes' original field list.
  - The "static X25519 key per device, registered at pairing" mechanism `sync-device-remove` depends
    on is **not** built here: `PairingSession` only handles the ephemeral ECDH. Registering (and
    persisting, as ops, in a `devices` table) each device's long-term static key is unstarted —
    `sync-device-remove` cannot land until it does.
  - "Snapshot then op tail to the new device" is unstarted: it needs `txtodo-store`'s `snapshots`
    table wired to the wire protocol, which is daemon-adjacent integration work, not pure
    crypto/state-machine.
- Not in this slice: the `devices` table, static-key registration/persistence, snapshot+ops
  streaming to a newly paired device, and every `@cli` subtask (`txtodo pair`, the QR rendering, the
  mutual yes/no confirmation prompt). These land as their own commits under the one-slice-per-session
  rule; `tasks/sync-pairing/todo.txt` tracks which subtasks remain.

## As built (2026-09-13, agent) — the `@cli` slice: `txtodo pair`

Verified first, since the notes above and `tasks/sync-pairing/todo.txt` still said `@cli` was
blocked on a missing daemon gRPC surface: that surface (`PairOffer`/`PairAccept`/`PairConfirmSas`,
`crates/txtodo-daemon/src/pairing_grpc.rs`) already existed (commit `81f3707`), fully unit-tested
in `pairing_grpc_tests.rs`. That blocker was stale; this slice is the actual CLI wiring.

- `crates/txtodo-cli/src/commands/pair.rs`: `txtodo pair [CODE]`. No `CODE` is the initiator
  (`run_offer`: calls `PairOffer`, renders the QR with the `qrcode` crate, prints the same text as
  a fallback code). `CODE` is the joiner (`run_join`: decodes the code locally, refuses on a
  detected `identity_mode` mismatch *before* calling `PairAccept` so a doomed pairing never
  consumes the offer's one-time nonce, then calls `PairAccept` for the real SAS, requires a typed
  "yes"/"y" — `is_explicit_yes`, anything else including EOF is no — before `PairConfirmSas`, then
  pulls a workspace snapshot). Matches notes.md's own command shape ("the other device runs
  `txtodo pair <code>` or scans") — one subcommand, not two.
- `crates/txtodo-cli/src/client.rs`: `pair_offer`/`pair_accept`/`pair_confirm_sas` wrappers, same
  shape as every other RPC method there.
- `crates/txtodo-cli/src/main.rs`: `Command::Pair { code: Option<String> }`, daemon-only (fails
  with `commands::history::NEEDS_DAEMON` in direct mode, same as `log`/`blame`/`undo`/`checkout`/
  `conflicts`). `commands::env` was split out of `main.rs` in the same commit — pure refactor, no
  behaviour change — because adding `Pair` pushed the file 9 lines over `budgets.json.fileLines`.
- **Wire addition**: `PairOfferResponse` gained a sixth field, `identity_mode` (`crates/txtodo-proto/
  proto/txtodo/v1/txtodo.proto`, regenerated), populated in `pairing_grpc.rs::response_of` from
  `Workspace::identity_mode()`. `docs/questions.md` Q6 already named this as the fix needed
  regardless of how the policy question resolves ("a new field on `PairOfferResponse`, the same
  shape as `group_id`'s") — adding it is infrastructure, not a policy guess.
- **Q6 (docs/questions.md, still open)**: `refuse_on_identity_mismatch` in `pair.rs` implements
  only the two states this task was told to implement — modes match (proceed), or the joiner's
  workspace has zero task lines yet, checked via `ListFiles`' `Progress.total` (proceed, matching
  Q6's own "nothing on disk to desync" reasoning for that case, and the *current, unquestioned*
  "no propagation yet" default: this device's `identity_mode` is never changed by pairing, here or
  anywhere) — and refuses, citing Q6 by name, on a real mismatch against an already non-empty
  workspace. **It does not, and must not, decide what a real mismatch should do beyond refusing**;
  that policy call is still the human's per Q6's own text. The joiner's side of the comparison is
  its CLI `config.toml` `identity_mode` (`Config::identity_mode()`), not the daemon's own
  authoritative, persisted mode — no RPC currently exposes the latter (`HealthResponse` doesn't
  carry it, and adding one felt like scope creep beyond what Q6 asked for); flagged below.
- **Snapshot**: `print_workspace_snapshot` calls `ListFiles` then `GetFile` per file after a
  confirmed local pairing — reusing the existing RPCs, since `pairing_grpc.rs` has no dedicated
  pairing-snapshot RPC (checked, per this task's own instructions). This is real, working code, but
  see the transport gap immediately below for what it cannot yet do.
- **The cross-device leg still does not exist, and this slice cannot build it.**
  `PairOffer`/`PairAccept`/`PairConfirmSas` only ever reach *this device's own* daemon
  (`pairing_grpc.rs`'s own module doc, unchanged by this session). Concretely, today:
  - `PairOfferResponse`/`PairAcceptRequest`/`PairResult`/`PairConfirmRequest` have no field for a
    joiner's public key or a sealed group key — even a human manually relaying text between two
    devices has nothing to carry, because the daemon never puts either on the wire in the first
    place (`PairingRegistry`'s `joiner_public_key`/`complete_as_initiator` relay-seam methods are
    `pub(crate)`, driven only by `pairing_grpc_tests.rs`).
  - So the initiator (`txtodo pair`) can show its QR/code but never a completed SAS or a
    confirmation prompt in this build; `run_offer` says this plainly instead of hanging or
    fabricating a SAS.
  - The joiner (`txtodo pair <code>`) gets and confirms a **real** SAS (both public keys are
    already in the code + its own fresh keypair, so this half needs no relay), but
    `PairConfirmSas`'s local confirmation can never observe the initiator's own confirmation or
    receive the real group key — `print_workspace_snapshot` therefore only ever shows the joiner's
    own pre-existing files, never anything that arrived from a peer. `run_join` says this plainly
    too.
  - This is the pre-existing `sync-lan-transport` gap (`crates/txtodo-sync/CLAUDE.md`'s own
    Invariants section: the iroh loopback-literal QUIC bug, and "not here yet: ... the `txtodo
    pair`/`txtodo device` CLI/daemon wiring"), not something introduced or fixable by a CLI-only
    slice. `tasks/sync-lan-transport/notes.md` owns closing it.
- **Tests**:
  - `crates/txtodo-cli/src/commands/pair_tests.rs` (unit): `PairingCode` JSON round-trip and its
    exact six keys, garbage-code rejection, `identity_mode_matches`'s four combinations plus an
    unrecognised string, and `is_explicit_yes`'s accept/reject cases (never a default yes).
  - `crates/txtodo-daemon/src/pairing_grpc_tests.rs`: `qr_payload_has_no_field_beyond_the_
    documented_five` → `_six`, asserting the new field and that a fresh workspace reports
    `"sidecar"`.
  - `crates/txtodo-cli/tests/pairing.rs` (integration, **not skipped**): two real `txtodod`
    processes, plain unix-socket IPC — no LAN transport involved at all, so the known iroh loopback
    bug (`tasks/sync-lan-transport/notes.md`) never comes into play. Asserts the initiator's QR +
    code + stated limitation; the joiner's real SAS, explicit "yes" (piped stdin), `PairConfirmSas`,
    and snapshot listing; a "no" aborting without ever calling `PairConfirmSas`; and the Q6 refusal
    (joiner has an existing task and a `config.toml` `identity_mode = "tagged"` against A's default
    `"sidecar"` offer). A true end-to-end two-daemon *network* pairing test is not written, and
    could not be — see the transport gap above; there is nothing to relay a peer's key over.
  - `cargo build --workspace`, `cargo test -p txtodo-cli -p txtodo-daemon`, `cargo clippy
    --workspace --all-targets -- -D warnings` (both crates), `cargo fmt --all --check`, and the
    file-length/boundaries scripts all pass (2026-09-13). `cargo deny check` has pre-existing,
    unrelated failures from `apps/desktop`'s `tauri` → `unic-ucd-*` chain (unmaintained advisories);
    neither `qrcode` nor `serde_json` appears in that output.
- **QR crate**: `qrcode` 0.14.1 (MIT OR Apache-2.0, allowed by `deny.toml`), `default-features =
  false` — the default features pull in `image`/`svg`/`pic` render backends this CLI never uses;
  the always-available `render::unicode::Dense1x2` renderer (half-block glyphs, no extra feature)
  is all a terminal needs. Chosen over rolling a QR encoder by hand (a solved, fiddly problem —
  Reed–Solomon error correction, mode/version selection) and over `fast_qr`/other alternatives
  because it is the most widely used pure-Rust QR crate with no `unsafe_code` and a small dependency
  footprint once `image` is off.
- Judgement calls, flagged for the human:
  - The code/QR payload is JSON (matching `pairing_wire.rs`'s actual, already-built wire shape),
    not the base32 this notes.md originally sketched (`## The QR carries no secrets` above) —
    `txtodo-cli` cannot depend on `txtodo-sync` (slice rule), so it cannot reuse `PairingOffer::
    to_code`/`from_code` (postcard + base32) even if it wanted to; JSON matching `pairing_wire.rs`
    is the only format the daemon's real `PairAccept` RPC actually accepts today.
  - The joiner's own `identity_mode` for the Q6 comparison is read from its CLI `config.toml`, not
    queried from its daemon's authoritative, persisted mode (no RPC exposes it) — a real mismatch
    between a stale `config.toml` and the daemon's actual mode would go undetected. Flagged rather
    than fixed by adding another RPC, to keep this slice to what Q6 and this task asked for.
  - There is no `PairReject`/cancel RPC in the proto. A declined SAS (`run_join`'s "no" path) simply
    never calls `PairConfirmSas`; the daemon's own `PAIRING_WINDOW_MS` and `MAX_CONCURRENT_PAIRINGS`
    close the window on their own. Good enough for "never send a false confirmation", but a human
    who wants to immediately retry pairing has to wait out the window rather than cancel it.
- Still not in this slice, and blocked on work outside a CLI-only task: real snapshot+ops delivery
  from an actual second device, and everything else `sync-lan-transport` owns (the group key
  genuinely landing on a joiner, the initiator ever displaying a real SAS). `tasks/sync-pairing/
  todo.txt`'s "Complete the parent line in `./todo.txt`" is deliberately left undone — this
  session's own instructions said not to touch the root `todo.txt`, and the transport gap above
  means it would be premature regardless.

## As built (2026-09-13, agent) — real cross-device pairing over the LAN transport

Closes the gap the `@cli` pass above named: `PairOffer`/`PairAccept`/`PairConfirmSas` now
genuinely cross the network between two real `txtodod` processes, over `sync-lan-transport`'s own
`Link`/`iroh` machinery, not a test-only seam. `txtodo pair` (initiator) shows a real SAS derived
from a real joiner's public key; `txtodo pair <code>` (joiner) receives the real sealed group key
and the initiator's real files.

- **A second iroh ALPN, not a new frame version** (`crates/txtodo-sync/src/endpoint.rs`,
  `lan_link.rs`): `PAIRING_ALPN` (`"txtodo/pairing/1"`) registered alongside the existing sync ALPN
  on the same bound `LanEndpoint`, so one endpoint accepts both a group-keyed sync connection and a
  pairing connection, told apart by `IrohLink::alpn()` — never by frame content, since a pairing
  connection has no group key to seal anything with in the first place. `frame.rs`'s frozen
  envelope (`Frame`, `PROTOCOL_VERSION`) is untouched; pairing frames use it as-is on a physically
  separate connection. `LanEndpoint::connect_pairing` dials with the new ALPN.
- **`pairing_relay.rs`** (`txtodo-sync`, transport-agnostic like the rest of the crate): `JoinerHello`
  (device/group/nonce/ephemeral pubkey/static pubkey/confirmed) and `InitiatorReply`
  (`Pending`/`Rejected`/`Grant(sealed)`), postcard-encoded inside `Frame`. New read-only
  `PairingSession` accessors (`group`/`nonce`/`is_handshaken`/`is_locally_confirmed`) let a network
  driver validate and drive a session without exposing its private fields to `txtodo-daemon`.
- **`pairing_lan.rs`** (`txtodo-daemon`, new): the actual relay driver.
  - Joiner: `spawn_joiner` (started by `pair_accept_impl` right after the local handshake) finds
    the initiator via `lan.rs`'s now-unfiltered sighting book (`pairing_lan_state.rs`'s
    `PairingLan` — pairing needs a peer by device id regardless of group, since groups deliberately
    differ pre-pairing), dials it on `PAIRING_ALPN`, and retries a short `JoinerHello`/
    `InitiatorReply` request-response burst every 500 ms — never one held-open connection, matching
    `IrohLink::recv`'s own idle-timeout design (`sync-lan-transport`'s "short bursts, not one
    held-open connection" pattern) — until a grant lands, is rejected, or `PAIRING_WINDOW_MS`
    elapses.
  - Initiator: `handle_incoming` (dispatched from `lan.rs`'s accept loop by ALPN) validates an
    incoming `JoinerHello`'s group/nonce against a read-only `PairingRegistry::snapshot` *before*
    touching any crypto state (a security fix beyond the original design: without this, any stray
    connection on the pairing ALPN during the window could burn the one-shot handshake slot), then
    drives `complete_as_initiator`/`mark_remote_confirmed`/`try_finalize_initiator` — the same
    relay-seam methods the prior pass built and only `pairing_grpc_tests.rs` drove.
  - `PairingLan::cache_grant`/`cached_grant`: the initiator's sealed grant is cached at the network
    layer once produced, because `try_finalize_initiator` clears `PairingRegistry`'s active session
    on success (by design, unit-tested behavior left unchanged) — a retried `JoinerHello` after a
    dropped reply still gets the same grant resent instead of hitting `NotActive`.
- **New `PairAwaitPeer` RPC** (`txtodo-proto`, regenerated bindings): the initiator's `txtodo pair`
  can't block the daemon on a real network wait, so this RPC never blocks either —
  `PairResult.sas` empty means "no peer yet", non-empty means the real handshake completed. The CLI
  loops it.
- **CLI** (`crates/txtodo-cli/src/commands/pair.rs`): `run_offer` polls `PairAwaitPeer` (bounded,
  ~125 s to match `PAIRING_WINDOW_MS`) before showing the real SAS; `run_join` polls
  `Health.lan_group_key_present` after confirming, then gives the LAN sync engine a short bounded
  courtesy wait before printing the snapshot. Both replace the previous "the transport hasn't
  landed" messaging.
- **Snapshot delivery: the existing sync engine, not a new RPC.** Once `Workspace::adopt_group_key`
  lands the group key, it also updates this workspace's own group id to the initiator's
  (already-built behavior, previously only exercised by `pairing_grpc_tests.rs`) — from there,
  `lan.rs`'s existing group-keyed `Session`/`Want`/`Ops` engine (unchanged) discovers the
  now-matching peer and replays its ops from genesis, the same whole-tree-from-empty mechanism
  `nested_ref_sync.rs` already proved. This matches the task's own "OR determine if
  `sync-lan-transport`'s own sync mechanism is the right vehicle" framing — it is; no dedicated
  pairing-snapshot RPC was built.
- **A real architectural gap this pass found and fixed**: `lan.rs`'s background task captures the
  workspace's group id *once* at startup into an immutable `Discovery` mDNS advertisement and
  `PeerTable`. `adopt_group_key` changing the workspace's group afterward went unnoticed by the
  already-running task — the joiner would hold the right group key but keep advertising (and
  filtering) under its old, pre-pairing group forever, so it could never be found by the initiator
  for ordinary sync. Fixed with a group-change check on the existing `RESYNC_INTERVAL` tick
  (`rebuild_on_group_change`): re-registers `Discovery` under the current group and rebuilds
  `PeerTable` when it differs from what was last advertised.
- **Two real bugs found only by actually driving the handshake** (manual two-process repro, then
  confirmed by the new tests below): `process_hello` checked the cached grant *after* requiring an
  active `PairingRegistry` session (backwards — fixed by checking the cache first), and
  `finish_joiner` never called `mark_remote_confirmed` on the *joiner's own* session before
  `adopt_group_key`'s `is_ready_to_send_key` check, so that check's `remote_confirmed` flag was
  always false on the joiner's side and `unwrap_grant` always refused with `NotConfirmed`.
  Receiving a non-empty `Grant` at all is itself proof the initiator's session was ready to send
  one, so it now doubles as that signal for the joiner.
- **Tests**:
  - `crates/txtodo-sync/src/pairing_relay_tests.rs`: wire round-trips for `JoinerHello`/
    `InitiatorReply`, a wrong-shape decode, an unknown frame version.
  - `crates/txtodo-daemon/tests/pairing_lan.rs` (the task's own required shape): two real `txtodod`
    processes, `PairOffer` -> `PairAccept` -> `PairConfirmSas` on both sides, no
    `DebugSetGroupKey`. Asserts identical SAS words on both real devices, `Health.
    lan_group_key_present` flips true on the joiner, and the joiner's real file converges to the
    initiator's real content (bounded polling, 30 s deadline — observed 5-17 s in practice; the
    existing MITM guarantee in `txtodo-sync`'s `pairing_tests.rs` is untouched and still passes).
  - `crates/txtodo-cli/tests/pairing.rs` rewritten: `txtodo pair` (initiator) now blocks on a real
    peer, so every test spawns it in the background (reading stdout progressively for the code
    line) instead of `.output()`. The main test seeds the initiator's workspace with a real task
    line and polls the joiner's own disk file until it matches — the real acceptance bar, exercised
    through the actual user-facing commands, not just RPCs.
  - `cargo build --workspace`, `cargo test -p txtodo-sync -p txtodo-daemon -p txtodo-cli
    -p txtodo-proto`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo fmt --all --check`, and the file-length/boundaries scripts all pass (2026-09-13).
- Judgement calls, flagged for the human:
  - The pairing relay's request/response burst carries no encryption of its own beyond what
    `PairingSession` already provides (an ephemeral/static public key is no more secret than the
    QR's own `x25519_pub`; the grant is already AEAD-sealed) — matches `lan_session.rs`'s own
    stance for the group-sync `Message` protocol, not a new judgement call, but worth restating
    since this is the first time that reasoning applies to key material crossing pre-pairing.
  - `rebuild_on_group_change` re-advertises by calling `Discovery::start` again (a fresh
    `mdns_sd::ServiceDaemon`) rather than mutating the existing one in place — `Discovery` exposes
    no "update my TXT record" method, and this is a one-time event per pairing, not a hot path.
  - The reverse leg of the static-key exchange (initiator learning the joiner's static key,
    flagged as unbuilt by the `sync-device-remove` pass) is now built too: `JoinerHello` carries
    the joiner's static public key in the clear (safe by the same reasoning as its ephemeral key),
    and the initiator registers it in its own `devices` table once both sides confirm
    (`pairing_lan.rs::register_joiner_device`) — not asked for explicitly by this task's brief, but
    a small, natural extension now that a real transport exists to carry it, and it makes the two
    sides' `devices` tables symmetric.
  - `PAIR_DEADLINE`/`AWAIT_PEER_TIMEOUT`/`CONVERGE_GRACE` are all generous, bounded constants (not
    guesses at a tight number) — real completion measured in single-digit seconds in this sandbox
    (mDNS discovery + the pairing burst's own retry cadence + one group-sync round), but a shared
    CI runner can be slower, and none of these ever hang indefinitely on a peer that never shows.
- Not in this slice, flagged as follow-up: a `PairReject`/cancel RPC still doesn't exist (unchanged
  from the `@cli` pass); a lost/dropped final `InitiatorReply::Grant` beyond the cache's lifetime
  (bounded only by the next `begin_offer` overwriting it, not an explicit TTL) has no separate
  retry path other than the joiner's own window expiring and the human retrying; `txtodo doctor`
  does not yet report "pairing in progress" as its own state (it already reports `paired`/
  `lan_group_key_present` before and after, which is the state that matters).
