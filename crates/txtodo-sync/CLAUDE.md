# txtodo-sync

## Purpose
Protocol, transports, pairing, crypto. Plan M4/M8.

## Public interface
- `Frame { version, body }` — the frozen envelope (`TXTO` · u16 LE version · u32 LE len · body);
  `Frame::{new, encode, decode, peek}`, `FrameError`, `PROTOCOL_VERSION` (= 2, bumped 1 -> 2 by
  task `daemon-workspace-session-multiplex`, root todo, stage 1), `MAX_FRAME_BYTES`.
- `Message::{Hello, Want, Ops, Ack, Greet}` (append-only variants — `Greet` added stage 2, index 4,
  no `PROTOCOL_VERSION` bump since appending a variant is exactly the safe case), `Message::{encode,
  decode, check_caps, workspace}`, `MessageError`, `Heads = BTreeMap<DeviceId, u64>`, `OriginRange`,
  `GroupId`, caps `MAX_OPS_PER_BATCH` / `MAX_WANT_RANGES` / `MAX_HEADS`. `Ops` carries
  `signatures: Vec<Signature>` parallel to `ops` (`sync-reject-tests`); a length mismatch is
  `MessageError::SignatureCount`. `Want`/`Ops`/`Ack`/`Greet` each also carry `workspace: u128` (task
  `daemon-workspace-session-multiplex`) — a raw ULID, not the typed `txtodo_store::WorkspaceId`, the
  same wire idiom `control.rs`'s `ControlMessage` already established (`txtodo-store` carries no
  `serde` dependency, and adding one so `Message` can derive `Serialize` on a typed `WorkspaceId`
  field is not worth it). `Message::workspace() -> Option<WorkspaceId>` converts back to the typed id
  (`None` for `Hello`) for a caller demuxing incoming messages by it. Goldens in
  `goldens/*.postcard` (regenerate with `TXTODO_UPDATE_GOLDENS=1 cargo test -p txtodo-sync`).
  **`Greet { workspace, heads }` (stage 2) is the per-workspace counterpart of `Hello` in a
  multiplexed world**: `Hello` now negotiates device+group exactly once per *link* (sent/consumed
  by `Session::link_hello`/`on_link_hello`, never per workspace — its own `heads` field is always
  an empty map now, dead weight kept only because the struct's layout is frozen); `Greet` is what
  each individually open workspace exchanges once that link handshake is done, carrying just that
  workspace's own heads (`Session::hello`/`on_hello`, per workspace).
- `want(local, remote) -> Vec<OriginRange>`, `advance(heads, run) -> Result<(), Gap>`.
- `Session` (task `daemon-workspace-session-multiplex`; stage 1 redesigned it from a one-workspace
  type, stage 2 split the link handshake out of it): a container of one sub-session per open
  `WorkspaceId`, multiplexing several workspaces' Want/Ack bookkeeping over one `(device, group)`
  peer relationship — `Session::new(device, group)` holds no workspace open yet and its link
  handshake unstarted; `open_workspace(id, heads) -> Result<(), SessionError>` opens one
  (idempotent-in-place for an already-open id; a genuinely new one past `MAX_OPEN_WORKSPACES` is
  `SessionError::TooManyWorkspaces`). `is_open`, `device`, `group`, `peer` (the peer's device id,
  learned from its link `Hello`, shared across every open workspace on this link) are
  workspace-agnostic accessors; `state(id)`, `heads(id)`, `wanted(id)` are per-workspace and
  `Result`-returning — an unopened or unknown id is `SessionError::UnknownWorkspace`, never a panic.
  **Link-level handshake** (stage 2, new): `link_hello(now_ms) -> Result<Message, SessionError>`
  builds our own `Hello` (refuses a second call, `SessionError::LinkAlreadyGreeted`);
  `on_link_hello(msg, now_ms) -> Result<Skew, SessionError>` consumes the peer's — checks group,
  protocol and clock skew (moved here from the old per-workspace `on_hello` in stage 1), requires
  our own `link_hello` already sent (`SessionError::LinkNotReady` otherwise), refuses a second peer
  `Hello` (`LinkAlreadyGreeted`), and a non-`Hello` message is `SessionError::NotAHello(&'static
  str)`; records `peer` on success. **Per-workspace** (stage 2: same names as stage 1, now
  `Greet`-based): each workspace's own state machine, `Idle → Greeted → Wanting → Importing →
  (Wanting | Idle)` (unchanged in spirit, living in `workspace_session.rs`'s crate-private
  `WorkspaceSession`), is driven by `hello(id) -> Result<Message, SessionError>` (builds our
  `Greet` for `id`), `on_hello(id, msg) -> Result<Message, SessionError>` (consumes the peer's
  `Greet`, returns our `Want` directly — the `Greeting{want, skew}` bundle stage 1 had is gone,
  since skew is a link-level fact now; requires the link handshake already done,
  `SessionError::LinkNotReady`, checked *after* confirming `id` is actually open so an unknown
  workspace is still the more specific `UnknownWorkspace`), `on_ops(id, msg,
  &BTreeMap<DeviceId, DevicePublicKey>)`, `committed(id, ranges)`. `on_ops` first checks `msg`'s own
  embedded `workspace` field matches `id` (`SessionError::WorkspaceMismatch` otherwise — a message
  routed to the wrong sub-session is refused, never silently misapplied), then runs `verify_batch`
  on `msg`'s ops/signatures before the wanted-range check — a bad signature or unrecognised device
  is `SessionError::Crypto` and never touches that workspace's `wanted`/`inflight`. `Session` never
  touches the group-key AEAD; a caller decrypts first (`sealed_ops::open_ops`) and only hands
  `Session::on_ops` an already-opened `Message`. **Wired into `txtodo-daemon`'s real multiplexed
  dispatch as of stage 2** — `lan_session_dispatch.rs::drive_shared_session` opens every routed
  workspace onto one `Session` and interleaves `Greet`/`Want`/`Ops`/`Ack` across all of them over
  one connection; see that crate's own `CLAUDE.md`.
- Crypto: `sign(op, &DeviceSigningKey) -> Signature`, `verify(op, &Signature, &DevicePublicKey)`,
  `verify_batch(&[Op], &[Signature], &BTreeMap<DeviceId, DevicePublicKey>)` (all-or-nothing);
  `DeviceSigningKey`/`DevicePublicKey`/`Signature` with `from_bytes`/`to_bytes`.
- `seal(version, SealFor, &GroupKey, plaintext)` / `open(version, group, workspace, &GroupKeys,
  sealed)` (`SealFor { group, epoch, workspace }` bundles the three so `seal` stays under the
  5-argument cap), `GroupKey`, `GroupKeys`, `CryptoError`, `MAX_RETAINED_KEY_EPOCHS`, header/AAD/
  nonce/tag byte consts. The clear header/AAD is `version || group || epoch || workspace` (ADR
  0021, task `daemon-shared-sync-link`, 2026-09-15): every workspace a device opens now shares one
  group key (`daemon-device-set-identity`), so now that several workspaces' traffic really is
  multiplexed over one shared `Link` (`daemon-workspace-session-multiplex` stage 2,
  `txtodo-daemon`'s `lan_session_dispatch::drive_shared_session`), `workspace` is what a receiver
  demuxes on (`peek_workspace`, read before a key is even looked up) and the AEAD tag is what stops
  a mislabelled batch (bug or active relay) from being silently routed into the wrong workspace's
  oplog — `CryptoError::WrongWorkspace` mirrors `WrongGroup`'s shape exactly, checked the same way,
  before the ciphertext is touched. This module's own binding — one `workspace` per sealed frame —
  did not need to change for real multiplexing: each outgoing `Message`, whichever workspace it is
  for, still becomes its own sealed `Frame`; a `Session` with several workspaces open just means
  more distinct sealed frames flow over the one link, each still bound to exactly one workspace.
- `sealed_ops.rs` (`sync-reject-tests`, M4; `workspace` field added by `daemon-shared-sync-link`):
  the actual op send/receive path, wiring the crypto above into real `Frame`s instead of leaving
  `Session` to call it. `seal_ops(ops, ranges, &DeviceSigningKey, &SealContext)` signs then seals
  (`SealContext { group, epoch, workspace, key }` bundles the four so the function stays under the
  5-argument cap, building an `aead::SealFor` from its own `group`/`epoch`/`workspace` to call
  `seal`); `open_ops(&Frame, GroupId, WorkspaceId, &GroupKeys, &BTreeMap<DeviceId,
  DevicePublicKey>)` opens then verifies, all-or-nothing, and its `Ok(Message)` is ready to hand to
  `Session::on_ops`. `SealedOpsError` wraps `CryptoError`/`MessageError`.
- Keystore (M4 `sync-keystore`): `KeyStore` trait (`get`/`put`/`delete`), `KeyId`
  (`DeviceSigning`/`DeviceStatic`/`Group(epoch)`), `Secret` (redacted `Debug`, zeroized on drop),
  `KeyStoreError`, `MAX_STORED_EPOCHS`. Backends: `MemoryKeyStore` (tests only), `FileKeyStore`
  (`create`/`open`, Argon2id + XChaCha20-Poly1305, params in the file header,
  `ARGON2_MEMORY_KIB`/`ARGON2_ITERATIONS`/`ARGON2_PARALLELISM`), `OsKeyStore` (`keyring` crate,
  `probe` for reachability). `resolve(mode, probe_os, make_os, make_file) -> (ResolvedBackend, Box<dyn
  KeyStore>)` implements `key_store = "auto" | "os" | "file"`; `KeyStoreMode`, `ResolvedBackend::name`.
- Pairing (M4 `sync-pairing`), transport-agnostic — no networking, just the crypto/state machine:
  `wordlist()` (the vendored EFF short list, `WORDLIST_LEN` = 1296, `WORDLIST_SHA256`);
  `transcript(protocol_version, Party, Party, GroupId) -> [u8; TRANSCRIPT_BYTES]` (`Party { device,
  public_key }`, canonically ordered by `DeviceId` so either side's own/peer view agrees);
  `sas_words`/`pair_key(shared_secret, transcript)` (HKDF-SHA256, `SAS_INFO`/`PAIR_KEY_INFO` distinct);
  `PairingOffer` (device, group, ephemeral X25519 public key, endpoint hint, nonce, `issued_at_ms` —
  no secrets) with `to_qr_bytes`/`from_qr_bytes`/`to_code`/`from_code` (base32); `NonceRegistry`
  (`issue`+`consume` for the initiator's own offer, `witness` for the joiner's replay/window check
  against the offer's own `issued_at_ms`), `PAIRING_WINDOW_MS`, `MAX_CONCURRENT_PAIRINGS` (= 1);
  `PairingSession::{offer, accept, complete, sas_words, confirm_local, confirm_remote, reject,
  is_ready_to_send_key, wrap_group_key, unwrap_group_key, wrap_grant, unwrap_grant, peer_device,
  group, nonce, is_handshaken, is_locally_confirmed}` (the last four, M4 `sync-pairing`'s LAN wiring
  pass, are read-only accessors a network relay driver needs to validate and drive a session
  without touching its private fields), `MAX_FAILED_SAS_CONFIRMATIONS`, `PairingError`.
  `PairingGrant { group_key, static_public }` is the
  normative confirmed-exchange payload — bundles the group key with the sender's long-term static
  public key so a caller cannot register one without the other; hand-written `Debug` redacts
  `group_key` only (`static_public` is not secret).
- Device static keys (M4 `sync-device-remove`, registered *at* pairing via `PairingGrant` above):
  `DeviceStaticSecret::{generate, from_bytes, to_bytes, public_key, diffie_hellman_with}`,
  `DeviceStaticPublic::{from_bytes, to_bytes}`, `DEVICE_STATIC_KEY_BYTES`. Long-term (unlike
  `pairing.rs`'s ephemeral per-handshake key) — generated once, stored via the keystore under
  `KeyId::DeviceStatic`.
- Rotation (M4 `sync-device-remove`), the crypto core only: `wrap_grant_for(new_key_bytes, epoch,
  &DeviceStaticPublic) -> WrappedGrant` (one ephemeral ECDH per recipient — the recipient's
  long-term key never doubles as an AEAD key directly), `open_grant(&WrappedGrant,
  &DeviceStaticSecret)`, `plan_rotation(current_epoch, new_key_bytes, &BTreeMap<DeviceId,
  DeviceStaticPublic>) -> BTreeMap<DeviceId, WrappedGrant>`, `validate_removal(removing,
  this_device, devices_before)` (never self, never the last device), `GRANT_INFO`, `RotationError`,
  `RemovalError`.
- `Link` trait (M4 `sync-lan-transport`, foundation only): `send(Frame) -> Result<(), LinkError>` /
  `recv() -> Result<Frame, LinkError>`, `Send` but not `Sync` (one link, one driver). `ChannelLink` +
  `channel_link_pair()` is the in-process implementation the loopback tests and the simulator use;
  `MAX_QUEUED_FRAMES` bounds each direction rather than growing without limit.
- `endpoint.rs` (M4 `sync-lan-transport`): `bind_local_endpoint()` — the one iroh `Endpoint`
  constructor (`presets::Minimal`, `RelayMode::Disabled`, `PortmapperConfig::Disabled`), `ALPN` and
  `PAIRING_ALPN` (M4 `sync-pairing`'s LAN wiring pass — a second ALPN registered on the same
  endpoint so one bound `LanEndpoint` accepts both a group-keyed sync connection and a pairing
  relay connection, told apart by `IrohLink::alpn()` rather than frame content). See Invariants for
  a known upstream connect/accept blocker, now confirmed to hit this constructor too. Kept only as
  the historical/test-only LAN-disabled shape — ADR 0024 dropped LAN transport entirely, so no real
  caller binds through here any more; see `relay.rs`.
- `relay.rs` (M8 `sync-relay-enable`, design §4.5): the relay-on twin of `endpoint.rs`. `RelayConfig
  { url, max_peers }` (`url` required — ADR 0024 removed the relay-off mode, so an empty url is
  `RelayError::EmptyUrl`, not a fallback to LAN), `MAX_RELAY_PEERS`, `RelayError` (`EmptyUrl` /
  `InvalidUrl` / `TooManyPeers` / `Bind`, validated never asserted — the config is external input).
  `build_endpoint(&RelayConfig) -> Result<Endpoint, RelayError>` binds `presets::Minimal` with
  `RelayMode::Custom(RelayMap::try_from_iter([url]))` (never iroh's own `Default`/`Staging`
  presets) and `PortmapperConfig::Disabled`, same `ALPN`/`PAIRING_ALPN` pair as `endpoint.rs`.
  `iroh` appears in this file, `endpoint.rs`, `lan_link.rs` and `holepunch.rs` only
  (`.claude/budgets.json`'s `allowedDeps`). Not yet wired into `txtodo-daemon`'s background sync
  loop (`lan.rs`'s relay counterpart) or exposed via `config.toml`/`--relay`/`txtodo doctor` — those
  are `sync-relay-enable`'s remaining todo items.
- `holepunch.rs` (M8 `sync-relay-enable`, design §4.5): connect/accept over a `relay.rs`-built
  endpoint, mirroring `lan_link.rs`'s split from `endpoint.rs`. Hole-punch-with-relay-fallback is
  not hand-rolled: iroh's own connection establishment already does it, so this module is one
  `Endpoint::connect`/`accept` call each, reusing [`IrohLink`] (now `pub(crate)`, not private —
  a `Link` over one QUIC stream is the same type whether the connection reached its peer via LAN or
  relay). `RelayEndpoint::{node_id_bytes, online, bind}` (`bind` takes a `GroupId` — every
  `connect` gates on the peer sharing it, refusing before dialing rather than after a handshake
  fails, same as LAN's `PeerTable::observe`), `connect(node, their_group)`, `accept()`.
  `HolepunchError` (`ForeignGroup` / `InvalidPeerId` / `Connect` / `Connection` / `NoIncoming` /
  `Stream`, same split as `LanError`: a peer's own protocol violation is `LinkError`'s job, not
  this type's). `RelayEndpoint::bind_insecure_for_test`/`relay::build_endpoint_insecure_for_test`
  are `#[cfg(test)]`-only (compiled out of every real build) so a test can dial
  `iroh::test_utils::run_relay_server`'s self-signed local relay; production code has no path to
  skipping certificate verification. The real-local-relay rendezvous test is `#[ignore]`d — the
  QUIC connection genuinely establishes (confirmed with `RUST_LOG=iroh=debug`) but `open_bi`/
  `accept_bi` never settle within two `iroh` endpoints sharing one process, the same class of
  same-process artifact `endpoint_tests.rs`/`lan_link.rs` document for LAN; real cross-process proof
  is `relay-converge-test`'s job (plan M8, its own ticket), same relationship as
  `lan_loopback_converge.rs` proving LAN for real.
- `discovery.rs` (M4 `sync-lan-transport`): `SERVICE_TYPE` (`_txtodo._udp.local.`), TXT keys
  `TXT_DEVICE`/`TXT_GROUP`/`TXT_PROTO`/`TXT_NODE` (never the group key; `TXT_NODE` is the
  advertiser's iroh `EndpointId`, opaque `[u8; 32]` here — this module still never names `iroh`),
  `Announcement`, `parse_announcement(&TxtProperties) -> Result<Announcement, AnnouncementError>`.
  `PeerTable` — the pure decision core, no sockets, no clock (`now_ms` injected): `new`/
  `with_limits`, `observe(announcement, addresses, now_ms) -> PeerEvent` (self/foreign-group/
  protocol filter, then debounce `DEBOUNCE_MS`, then `MAX_LAN_PEERS`, in that order), `remove
  (device)`, `len`/`is_empty`. `backoff_ms(attempt) -> u64` (`250ms * 2^attempt`, capped at
  `MAX_BACKOFF_MS`). `Discovery::start(device, group, node, host_name, port)` owns the `mdns-sd`
  daemon and registers our advertisement (`enable_addr_auto`); `browse()` returns `BrowseEvents`,
  an async `recv() -> Option<Sighting>` that filters raw `ServiceEvent`s down to resolved,
  parseable sightings — no caller outside this module ever names `mdns_sd::ServiceEvent`.
- `lan_link.rs` (M4 `sync-lan-transport`, daemon-wiring pass): the real `iroh`-backed `Link`.
  `LanEndpoint::bind()` wraps `bind_local_endpoint`; `node_id_bytes()`/`advertise_port()` feed
  `Discovery::start`; `connect(node, addrs)` (prefers non-loopback candidates, falling back to
  loopback only when nothing else was advertised — see Invariants; builds an `EndpointAddr`,
  opens the one bidirectional stream this protocol runs over) and `accept()` (waits one incoming
  connection, accepts that same stream) both return `IrohLink`. `connect_pairing(node, addrs)` (M4
  `sync-pairing`'s LAN wiring pass) is `connect`'s pairing-ALPN twin. `IrohLink` implements `Link`
  synchronously by `block_on`-ing the async stream ops on a captured `tokio::runtime::Handle` — it
  must run on a dedicated driver thread (`spawn_blocking`, never a plain tokio task), matching
  `Link`'s own "one link, one driver" contract. `alpn()` reports which of `ALPN`/`PAIRING_ALPN` the
  connection negotiated, so an accept loop can route it without ever naming an `iroh` type. `recv()`
  reports `LinkError::Closed` after `IDLE_TIMEOUT` (750 ms) of silence too, not only on a real close
  — by design, so a caller (the daemon) is expected to run short-lived sessions and redial
  periodically rather than hold one connection open for a whole pairing's lifetime; see Invariants.
  `LanError` names what failed. `iroh` appears only in this file and `endpoint.rs`; `txtodo-daemon`
  never names an `iroh` type (check `.claude/budgets.json`'s `allowedDeps`).
- `pairing_relay.rs` (M4 `sync-pairing`'s LAN wiring pass): the daemon-to-daemon relay's own wire
  messages, moved over `Link` on `PAIRING_ALPN` rather than the group-keyed `Message`/`Session`
  protocol (no group key exists between the two devices yet — that is the whole point of pairing).
  `JoinerHello` (device/group/nonce/ephemeral pubkey/static pubkey/confirmed) and `InitiatorReply`
  (`Pending`/`Rejected`/`Grant(sealed)`), postcard-encoded inside the same frozen `Frame` envelope
  `Message` uses — the separation is the ALPN, not a new frame version. `PairingRelayError`.
- `lan_op_signing.rs` (M4 `sync-lan-transport`): `derive_group_op_signing_key(&GroupKey) ->
  DeviceSigningKey`, `LAN_OP_SIGN_INFO`. HKDF-derives the LAN path's per-op signing keypair from
  the group key itself, so `Session::on_ops`'s signature check has *something* real to verify
  against — but every device sharing the group key derives the identical keypair, so this is
  **not** per-device attribution, only a second proof of "holds the group key" (the AEAD seal
  already proves that). Real per-device attribution needs a `DevicePublicKey` distributed through
  pairing and stored in the devices table — neither exists yet, see below.
- Not here yet: the `devices` table storing each peer's `DevicePublicKey` (only `DeviceStaticPublic`,
  the X25519 pairing key, is stored today — `lan_op_signing.rs`'s stand-in exists because of this
  gap), the daemon-level rotation sequencing ("close the epoch before announcing the removal"), and
  `ops.signature` in `txtodo-store` (still an unpopulated column — the LAN pass signs on the wire,
  not at rest). Real pairing *does* now cross the LAN transport (M4 `sync-pairing`'s LAN wiring
  pass, `pairing_relay.rs` here plus `txtodo-daemon`'s `pairing_lan.rs`) — snapshot-then-ops
  transfer to a newly paired device is real too, via `lan.rs`'s existing group-keyed sync engine
  once pairing adopts a shared group id/key, not a dedicated transfer RPC; see
  `txtodo-daemon/CLAUDE.md`.

## Invariants
- Known upstream blocker (2026-09-12, confirmed on macOS and Linux, not a sandbox artifact;
  **corrected 2026-09-13, corrected again the same day**): `noq-proto` 1.3.0 (vendored by `iroh`
  1.2.0) refuses a QUIC connection between two `Endpoint`s that live in the **same process** —
  logged as `network_path=(local: X, remote: [::ffff:X]:_)` and a `refuse()`, regardless of
  whether `X` is `127.0.0.1` or a real interface address (both were tried; both fail identically),
  and regardless of portmapper/dual-stack settings (also ruled out). Two intermediate, now-
  superseded theories are recorded and corrected in `endpoint_tests.rs`'s doc comments for anyone
  re-deriving this: first "both ends bind literally to `127.0.0.1`", then "any same-host
  connection" — both wrong. The actual precondition is same-*process*, confirmed by
  `txtodo-daemon`'s `tests/lan_loopback_converge.rs`: two real, separate `txtodod` processes on
  this same host connect and sync for real, repeatedly, with sub-millisecond measured convergence.
  `endpoint_tests.rs`'s two single-process tests stay `#[ignore]`d as same-process regression
  checks rather than deleted; nothing about real LAN sync is blocked on fixing them.
- Discovery never leaks the group key, only its id (`TXT_GROUP`); `PeerTable::observe` checks
  self-advertisement, then group, then protocol version, before debounce or the peer-table bound —
  a foreign-group peer can never consume a `MAX_LAN_PEERS` slot. `parse_announcement` never panics on
  a foreign program's TXT record under our service type; a missing or malformed field is a typed
  `AnnouncementError` naming which field.
- Every message versioned, authenticated, encrypted. Keys only in keystore.
- A device key and a group key are **injected**, never read here: `sign`/`seal` take them as arguments,
  so tests use fixtures and the keystore owns I/O. `seal` draws its nonce straight from `getrandom`;
  there is no RNG seam for the simulator's seeded PRNG to reach.
- Signature is per op and durable (`Op::signing_bytes` excludes nothing: the signature is never a
  field of `Op`); the AEAD is per batch and stripped at import. `verify_batch` is all-or-nothing.
- `sealed_ops::open_ops` opens before it decodes and decodes before it verifies: a wrong/absent
  group key never reaches `Message` parsing, and a bad signature never reaches `Session`.
  `Session::on_ops` re-verifies regardless of what its caller already checked — it does not trust a
  caller that skipped `open_ops` — and does so before touching `wanted`/`inflight`, so a rejected
  batch changes nothing about session state.
- Sealed header is `version || group || epoch || nonce`, and `version || group || epoch` is the AAD,
  so a foreign group/version or a tampered header fails rather than decrypting into something
  plausible. An unknown epoch is a typed error naming it, never a try-every-key loop.
- Failure is validated, never asserted: one `CryptoError`, every variant names the epoch/device/group.
- `Frame`'s layout never changes; `Message` variants are only ever appended; a version we do not
  speak is a typed error that consumes nothing. Every wire collection has a cap checked on both
  paths; a hostile length is refused before allocation.
- `Ack` carries committed runs, never received ones; `advance` refuses a run that leaves a hole.
- Transport- and store-agnostic: no sockets, no SQLite; the caller moves frames and commits ops.
- `key_store = "auto"` never writes a key file on its own: an unreachable OS backend under `auto` is
  `KeyStoreError::AutoNeedsChoice`, not a silent fallback to `FileKeyStore`. No test in this crate
  calls the real `keyring` backend (would pop a Keychain dialog in CI); `resolve`'s OS-availability
  check is always injected, and `OsKeyStore` itself is exercised only by inspection, not by a test.
- `FileKeyStore` never `Debug`s or logs its derived key; a permissive file (group/world-readable) is
  refused, never `chmod`'d back. `create` refuses to replace an existing file (checked, then closed
  against the TOCTOU race with `hard_link` rather than `rename`, which would silently replace).
- The pairing SAS commits to both devices' identities and ephemeral keys via `transcript`: an active
  MITM running two independent handshakes gets two different transcripts and thus two different SAS
  (`pairing_tests::mitm_relay_running_two_handshakes_produces_two_different_sas`). `SAS_INFO` and
  `PAIR_KEY_INFO` are distinct `HKDF-Expand` labels from the same extract step — never the same
  derived bytes for two purposes. The group key moves only once both `confirm_local` and
  `confirm_remote` are true and the window is not `closed`; a one-sided confirmation transfers
  nothing. A nonce is single-use whether the attempt succeeds or fails: the initiator tracks its own
  offer with `issue`/`consume`; the joiner, which never issued it, uses `witness` against the
  offer's own `issued_at_ms` instead.
- `Link` is the only place a real transport may ever be wired in; `Session`/`Message`/`Frame` never
  see a socket directly. `ChannelLink` closes its outbox on `Drop`, so a peer blocked in `recv`
  learns the other side is gone rather than blocking forever. `IrohLink::send`/`recv` block a
  dedicated driver thread on a captured `tokio::runtime::Handle`; calling either from a plain tokio
  task (rather than `spawn_blocking`) would starve the runtime, not just this one link.
- `IrohLink::recv`'s `IDLE_TIMEOUT` (750 ms) makes every LAN session short-lived by design: once
  both sides go quiet the link reports `Closed` and the daemon's periodic redial opens a fresh one,
  which is what lets a local edit made *after* an earlier sync round still converge quickly without
  this crate needing any "watch the store for changes" plumbing of its own. The tradeoff — a new
  QUIC handshake roughly every second for as long as two daemons stay paired and on the same LAN —
  is a known cost of this M4-scoped design, flagged for a human: a push/notify model would avoid it
  but is real additional work, not attempted this pass.
- `LanEndpoint::connect` prefers non-loopback candidate addresses, falling back to loopback only
  when nothing else was advertised (real two-process testing on one host sometimes resolves only a
  loopback address for a peer before its real interface address is known — refusing it outright
  made LAN sync flaky in exactly that situation). This is unrelated to the same-process bug above,
  which is about which process the two `Endpoint`s live in, not which address family is dialed.
- A rotation grant is sealed with a fresh ephemeral keypair per recipient, never the recipient's
  static key as an AEAD key directly (`GRANT_INFO` is a distinct `HKDF-Expand` label from
  `SAS_INFO`/`PAIR_KEY_INFO`); the epoch is bound as AEAD associated data, so a grant for one epoch
  cannot be relabelled as another. `validate_removal` is checked before any crypto runs — a removal
  refusal never touches key material.
- May depend only on: txtodo-model, txtodo-store.
