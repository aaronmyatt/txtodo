# `txtodo device remove` rotates the group key (plan M4)

Plan M4: "rotates the group key; remaining devices re-encrypt nothing (old ops stay under old key,
kept for history; new ops use new key)." Epoch mechanics are in
[sync-crypto-envelope](../sync-crypto-envelope/notes.md).

## Say the true thing in the CLI output

Removal does **not** un-share history. The removed laptop already holds every op ever synced to it,
in plaintext, in its own SQLite file. Rotation stops it reading *future* ops and nothing else.

That sentence belongs in the command's output, not only in these notes. A security feature that the
user believes does more than it does is worse than no feature: someone will remove a stolen device
and think the contents are safe. One line, every time:

```
Rotated to key epoch 4. The removed device keeps the history it already synced;
rotation only protects ops made from now on.
```

## The gap: rotation needs a per-device static key, pairing gives an ephemeral one

To hand epoch N+1 to the remaining devices you must encrypt it *to each of them individually* — the
old group key is exactly what you are trying to stop using, so it cannot wrap the new one.

[sync-pairing](../sync-pairing/notes.md) derives an **ephemeral** X25519 secret and throws it away.
Nothing today gives device B a long-term public key that device A can encrypt to later. Two ways:

- **A — register a static X25519 public key per device at pairing.** Store it in `meta`/a `devices`
  table, replicated as ops. Rotation writes one wrapped copy of the new key per remaining device.
  A device offline at rotation time finds its grant waiting when it returns.
- **B — re-pair every device after a removal.** No new key material, no code. Unusable with more
  than two devices, and it makes removing a device feel like a punishment.

Take **A**. Note that it changes [sync-pairing](../sync-pairing/notes.md) — the static key has to be
registered *there*, so the two tasks land in that order or rotation has nothing to wrap to.

## Ordering: close the epoch before you announce the removal

Between "user pressed remove" and "every device has epoch N+1", ops are still being made. Anything
emitted under epoch N in that window is readable by the removed device. So:

1. Mark epoch N closed **locally, first**. From this instant no new op is sealed under N.
2. Generate N+1, write one wrapped grant per remaining device.
3. Stop advertising to / accepting sessions from the removed device.

Doing (3) first feels natural and is wrong: it leaves a window where you are still sealing under a
key the removed device holds while believing it is cut off.

## Guards

- **Never remove yourself.** `device remove <this device>` is refused with a message pointing at
  whatever the "leave the group" story is — which is not this task.
- **Never remove the last device.** A group with no devices has an unreachable key.
- **Removal is idempotent.** Removing an already-removed device rotates nothing and says so.
- Removal requires explicit confirmation naming the device (CLAUDE.md: outward-facing and hard to
  reverse), and `--yes` exists for scripts but is not the default.
- `MAX_RETAINED_KEY_EPOCHS` from the crypto task still holds; a group that rotates past it loses
  the ability to read its oldest ops, so the cap must be justified, not just chosen.

## Also needed

`txtodo device list` — id, name, last seen, key epoch, whether it is this device. Removal without a
way to see what you are removing is not a usable command, and the id is a ULID nobody types from
memory.

## Tests

- After rotation, an op sealed by a remaining device does not open under the old key.
- The removed device receives no grant for the new epoch — assert on the grant set, not on a failed
  decrypt, so the test fails for the right reason.
- A device offline during rotation comes back, finds its grant, and reads new ops.
- Ops sealed *before* the rotation still open, under the retained old epoch.
- Removing yourself, removing the last device, and removing twice each fail or no-op distinctly.

## As built (2026-09-12, agent) — the crypto core only, in `txtodo-sync`

- `crates/txtodo-sync/src/device_static.rs`: `DeviceStaticSecret`/`DeviceStaticPublic`, an X25519
  keypair generated once at pairing (`x25519_dalek::StaticSecret`, reusable — unlike the ephemeral
  key `pairing.rs` uses for the handshake itself, this one is meant to be reused for every future
  rotation). `x25519-dalek`'s own `zeroize` feature (on by default) wipes the secret on drop.
- `crates/txtodo-sync/src/pairing_grant.rs`: `PairingGrant { group_key, static_public }` — the
  normative payload for `PairingSession::wrap_group_key`/`unwrap_group_key` (via the new
  `wrap_grant`/`unwrap_grant` convenience methods added to `PairingSession`), bundling the group
  key with the sender's static public key so integrating this later cannot register one without
  the other. This directly answers subtask 1 ("register a long-term X25519 public key per device
  during pairing... must land before rotation has anything to wrap to") — it now does, at the
  protocol level; `pairing_tests::pairing_also_registers_each_sides_static_public_key` exercises
  the full exchange both directions.
- `crates/txtodo-sync/src/rotation.rs`: `wrap_grant_for`/`open_grant` (one ephemeral ECDH per
  recipient, HKDF-SHA256 wrap key salted with both public keys, epoch bound as AEAD associated
  data so a grant cannot be relabelled to another epoch), `plan_rotation` (one grant per remaining
  device, epoch + 1), `validate_removal` (never self, never the last device — checked before any
  crypto runs, per the notes' "removal is refused... hard to reverse" framing).
- **`MAX_RETAINED_KEY_EPOCHS` justified against rotation rate** (subtask asked for this rather than
  picking a number): rotation only happens on `device remove`, not on a schedule — for a personal
  or small-team todo group, that is a rare, deliberate action (losing a laptop, someone leaving a
  shared list), realistically at most a handful of times a year. 16 retained epochs
  (`sync-crypto-envelope`'s constant, reused here rather than duplicated) covers many years of
  removals before the oldest history becomes unreadable, with headroom for a group that churns
  devices unusually often. If real usage ever shows faster churn, the number is one named constant
  to change, not a scattered assumption.
- `cargo test -p txtodo-sync --lib` (115 tests), `cargo clippy -p txtodo-sync --all-targets -D
  warnings`, `check-boundaries.sh`, `check-file-length.sh` all clean. `cargo deny check` shows an
  unrelated `advisories` failure from a concurrent session's `apps/desktop` (tauri) addition — not
  from anything added here (no new dependency; `x25519-dalek` gained the already-default
  `getrandom` feature it needed for `StaticSecret::random()`).
- Not in this slice (all daemon/store/CLI, deliberately avoided — a concurrent session was actively
  building M6/M7 in `crates/txtodo-daemon/` throughout this work): the `devices` table itself
  (persisting each peer's `DeviceStaticPublic`, replicated as ops), the sequencing rule ("close the
  epoch locally first, then generate N+1, then stop accepting the removed device" — this crate has
  no notion of "accepting a session" or "the current epoch" as mutable state to sequence),
  `txtodo device list`/`txtodo device remove` CLI, and the six `@test` subtasks that need a real
  `KeyStore` + `devices` table wired together (this crate's own unit tests cover the crypto
  primitives those integration tests would exercise, but not the sequencing itself).

## As built (2026-09-13, agent) — the `devices` table, daemon wiring, and the CLI

Picks up exactly where the previous pass left off: the crypto (`wrap_grant_for`/`open_grant`/
`plan_rotation`/`validate_removal`) is unchanged and reused as-is, nowhere reimplemented.

- `crates/txtodo-store/migrations/0006.sql` (schema now 6) + `crates/txtodo-store/src/devices.rs`:
  the `devices` table — `device`, `name`, `static_public` (32 bytes, plain — this crate cannot
  depend on `txtodo-sync`'s `DeviceStaticPublic`), `paired_at`, `last_seen`, `last_known_wall`
  (the peer's own clock reading, `NULL` until observed — feeds `model-hlc-skew-guard`'s doctor
  line), `key_epoch`, `removed_at`. `register_device` (upsert, un-tombstones a rejoining device —
  same idiom as `upsert_fingerprint`), `list_devices`, `device`, `remove_device` (idempotent
  tombstone), `set_device_key_epoch`. Removed rows are kept, never deleted, like every other table
  here — the wire boundary decides visibility.
- Pairing now actually registers a peer (`crates/txtodo-daemon/src/pairing_state.rs`,
  `crates/txtodo-daemon/src/workspace.rs::adopt_group_key`): `try_finalize_initiator`/
  `adopt_group_key` were still calling the bare `wrap_group_key`/`unwrap_group_key` from
  `sync-pairing`, not the `PairingGrant`-bundling `wrap_grant`/`unwrap_grant` this task's own crypto
  pass built for exactly this purpose. Switched them over: the initiator now sends its own
  `DeviceStaticPublic` alongside the group key, and the joiner's `adopt_group_key` registers it in
  its `devices` table. `pairing_grpc_tests.rs` gained
  `pairing_registers_the_initiators_static_public_key_in_the_joiners_devices_table`, driven the
  same in-process, no-transport way every other test in that file already is (relay-seam methods,
  not a real socket).
  - **Known, explicitly scoped gap**: only the joiner registers the initiator. The reverse
    direction (the initiator learning the joiner's static key) needs a second `PairingGrant` to
    flow back, and the only place bytes cross between two real daemons is the LAN transport
    (`sync-lan-transport`), which does not exist yet (see that task's own notes). The pairing
    *session* already supports both directions symmetrically
    (`pairing_also_registers_each_sides_static_public_key` in `txtodo-sync`'s own tests proves
    it) — this is a daemon-wiring gap, not a crypto one, and it means a 2-device group only has
    the *joiner's* devices table populated until a transport exists to carry the return trip.
- `crates/txtodo-daemon/src/device_remove.rs` — `Workspace::remove_device(target, now_ms)`:
  `validate_removal` first (never self, never the last device — devices_before is peer-count + 1,
  since the table holds peers only, never a self-row), idempotent tombstone, generates a fresh
  epoch key and calls `plan_rotation` for whatever peers remain (skipped, not treated as an error,
  when removing the last peer leaves a solo group — nobody to grant to), stores the new epoch's key
  under this workspace's own keystore, advances `meta`'s `group_key_epoch`, tombstones the target,
  advances each remaining peer's `key_epoch` column. Sequencing matches the task notes: the new
  epoch lands locally *before* the target row is tombstoned.
  - **Known gap, flagged for the human**: `plan_rotation`'s `WrappedGrant`s are computed for real
    (proving the crypto integrates) but not persisted or delivered anywhere — there is no "pending
    grants" store. "A device offline during rotation comes back and finds its grant" (this task's
    own acceptance criterion) needs either the LAN transport or a new store this task deliberately
    did not add (the brief asked for the `devices` table, not a second one). Each remaining
    device's `key_epoch` column records what this device *believes* it has minted a grant for, not
    confirmed delivery.
  - Found and fixed a real deadlock while writing `device_remove_tests.rs`: an earlier version held
    the store's `std::sync::Mutex` across a call into another `Workspace` method that re-locked it
    (`Mutex` is not reentrant). Every lock is now scoped to a block. `device_remove_tests.rs` (6
    tests): self-removal refused, last-device-in-a-solo-group refused, unknown device is a no-op
    (not an error), a real two-peer removal rotates the epoch/tombstones/advances the remaining
    peer, removing the sole remaining peer still rotates (with `grants_minted: 0`), and idempotent
    re-removal rotates nothing.
- `crates/txtodo-proto`: `Device`/`DeviceListRequest`/`DeviceListResponse`/`DeviceRemoveRequest`/
  `DeviceRemoveResponse`/`SkewStatus` messages, `DeviceList`/`DeviceRemove` RPCs.
  `crates/txtodo-daemon/src/devices_grpc.rs` wires them (same split as `tokens.rs`/`pairing_grpc.rs`);
  `DeviceRemoveResponse.message` always carries the "does not un-share history... only ops made from
  now on" caveat verbatim, per this task's own "say the true thing" section.
- `txtodo device list|remove <id> [--yes]` (`crates/txtodo-cli/src/commands/device.rs`): list shows
  id/name/last-seen/key-epoch/is-self/clock-skew; remove requires typing the device's id back to
  confirm (CLAUDE.md: outward-facing, hard to reverse) unless `--yes`, then prints the daemon's own
  caveat message. Covered by `crates/txtodo-cli/tests/daemon_mode.rs` (spawns a real `txtodod`):
  list-before-pairing, last-device-refused (with `--yes`, proving the guard isn't just the prompt),
  and an EOF on stdin refusing an unconfirmed removal rather than hanging or defaulting to yes.
- **Not attempted, and explicitly out of scope for this pass** (per the task brief): a real
  two-`txtodod`-process integration test. Everything above is exercised either as a daemon-crate
  unit/in-process test (pairing registration, removal/rotation) or a single-daemon CLI integration
  test (the guard paths) — nothing here spins up two live daemons on a real or simulated network.
  The six `@test` subtasks from the original notes that specifically asked for that
  (`sync-loopback-converge`-style, real `KeyStore` + `devices` table across two processes reading
  new-epoch ops) still need `sync-lan-transport` before they can exist as anything but the
  in-process approximations above.
- `cargo build --workspace`, `cargo test -p txtodo-store -p txtodo-daemon -p txtodo-cli` (includes
  `tests/crash.rs`, which spawns the real binary and kills it mid-write), `cargo clippy --workspace
  --all-targets -D warnings`, `cargo fmt --all --check` all clean.
