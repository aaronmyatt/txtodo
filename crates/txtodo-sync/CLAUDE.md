# txtodo-sync

## Purpose
Protocol, transports, pairing, crypto. Plan M4/M8.

## Public interface
- `Frame { version, body }` — the frozen envelope (`TXTO` · u16 LE version · u32 LE len · body);
  `Frame::{new, encode, decode, peek}`, `FrameError`, `PROTOCOL_VERSION`, `MAX_FRAME_BYTES`.
- `Message::{Hello, Want, Ops, Ack}` (append-only variants), `Message::{encode, decode,
  check_caps}`, `MessageError`, `Heads = BTreeMap<DeviceId, u64>`, `OriginRange`, `GroupId`,
  caps `MAX_OPS_PER_BATCH` / `MAX_WANT_RANGES` / `MAX_HEADS`. Goldens in `goldens/*.postcard`.
- `want(local, remote) -> Vec<OriginRange>`, `advance(heads, run) -> Result<(), Gap>`.
- `Session` — `Idle → Greeted → Wanting → Importing → (Wanting | Idle)` via `hello`, `on_hello`
  (group, protocol, `Skew` guard), `on_ops`, `committed`; `SessionError`, `Greeting`.
- Crypto: `sign(op, &DeviceSigningKey) -> Signature`, `verify(op, &Signature, &DevicePublicKey)`,
  `verify_batch(&[Op], &[Signature], &BTreeMap<DeviceId, DevicePublicKey>)` (all-or-nothing);
  `DeviceSigningKey`/`DevicePublicKey`/`Signature` with `from_bytes`/`to_bytes`.
- `seal(version, group, epoch, &GroupKey, plaintext)` / `open(version, group, &GroupKeys, sealed)`,
  `GroupKey`, `GroupKeys`, `CryptoError`, `MAX_RETAINED_KEY_EPOCHS`, header/AAD/nonce/tag byte consts.
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
  is_ready_to_send_key, wrap_group_key, unwrap_group_key, wrap_grant, unwrap_grant, peer_device}`,
  `MAX_FAILED_SAS_CONFIRMATIONS`, `PairingError`. `PairingGrant { group_key, static_public }` is the
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
  constructor (`presets::Minimal`, `RelayMode::Disabled`, `PortmapperConfig::Disabled`), `ALPN`. See
  Invariants for a known upstream connect/accept blocker, now confirmed to hit this constructor too.
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
  connection, accepts that same stream) both return `IrohLink`. `IrohLink` implements `Link`
  synchronously by `block_on`-ing the async stream ops on a captured `tokio::runtime::Handle` — it
  must run on a dedicated driver thread (`spawn_blocking`, never a plain tokio task), matching
  `Link`'s own "one link, one driver" contract. `recv()` reports `LinkError::Closed` after
  `IDLE_TIMEOUT` (750 ms) of silence too, not only on a real close — by design, so a caller (the
  daemon) is expected to run short-lived sessions and redial periodically rather than hold one
  connection open for a whole pairing's lifetime; see Invariants. `LanError` names what failed.
  `iroh` appears only in
  this file and `endpoint.rs`; `txtodo-daemon` never names an `iroh` type (check
  `.claude/budgets.json`'s `allowedDeps`).
- Not here yet: the `devices` table (persisting each peer's `DeviceStaticPublic` — this crate only
  produces/consumes the bytes, never stores them), the daemon-level rotation sequencing ("close the
  epoch before announcing the removal"), snapshot-then-ops transfer to a newly paired device, real
  pairing over the LAN transport (the daemon's own `sync-loopback-converge` test pairs through a
  guarded test-only seam instead — see `txtodo-daemon/CLAUDE.md`), and per-op signature transport
  (the wire's `Message::Ops` carries `Op` values with no accompanying `Signature`; `ops.signature`
  in `txtodo-store` is an unpopulated column today — `sync-reject-tests`/`sync-crypto-envelope`'s
  concern, not wired end-to-end by this crate yet).

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
