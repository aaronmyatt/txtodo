# peer-prune: drop stale peers on our own

## Goal

- A peer we have not heard from in a while is dropped from the sync group by the daemon itself,
  through `device remove` (row tombstoned, group key rotated). Coming back means pairing again.
- Why: on 2026-10-17 one parked peer (01M3ZKW45S, paired 2026-10-03, last seen that day) held
  `SyncStatus.pending_ops` at 3930 and growing. It holds no key we share, so it can never sync
  without a re-pair anyway.
- Decided 2026-10-17 (human): option A, pruned = automatic `device remove`, key rotation included.
  Rejected B (a saved, undoable "retired" mark, no rotation): the human wants peers dropped early,
  since re-pairing is always there.

## Vocab

- quiet: keys match, not heard from lately. Shown as lag. Not a failure.
- parked: 3 `wrong_group` opens in a row (`peer_keys.rs`). In memory; dials skip it; the devices
  row is kept; it undoes itself on re-pair, our group change, an in-group LAN sighting, or restart.
- stale (new): a verdict. Quiet past `prune_quiet_after`, or parked and not seen past
  `prune_parked_after`. Keyed off `last_seen` (on disk). Parking is in memory, but it comes back
  within ~30 s of a restart, so "parked and last_seen old" is still knowable.
- pruned (new): the daemon ran `device remove` on a stale peer by itself.
- removed: the same end state, run by a human.

## Design

- Defaults (agent pick, "agro" per the human): parked and not seen for 1 day; quiet for 7 days.
  Config `[sync] prune_parked_after`, `prune_quiet_after`; `0` turns that rule off.
- Never prune self. Pruning the last peer is allowed (`grants_minted` 0 already covers it).
- Who prunes: every device, on its own. Each keeps its own `devices` table (peers do not share it;
  ADR 0029's `OwnDevices` vouch list only says which peers are own) and prunes a peer once its own
  `last_seen` for it passes the threshold. Decided 2026-10-18 (human).
- What that costs: the group key is shared, so one device's prune rotates it for everyone. The
  pruned peer gets no grant, so it cannot read new ops from any peer, even one that still hears it.
  Agent pick (2026-10-18): a receiver follows the grant. A peer the grant leaves out is dropped
  here too, marked "dropped by <device>". Rejected: per-sender keys (each device rotates only its
  own sending key), which keep prunes truly local but rebuild the crypto.
- Two devices pruning at once mint two epoch N+1 keys. Pick: the grant from the lower device id
  wins; the other device adopts it and re-sends. The ADR line settles both for real.
- Coming back: a re-pair upserts the row with `removed_at = NULL` (`identity_store.rs`
  `UPSERT_DEVICE`), so the row side already works. Needs an end-to-end test.

## Blockers found 2026-10-17 (land before the prune loop)

- Rotation is half built. `device remove` advances `group_key_epoch`, and the control channel
  seals under it (`control_session.rs:44`), but the grants are minted and never delivered
  (`device_remove.rs` module doc). So any removal today breaks the control channel with the
  healthy peers (`unknown_epoch`). Do not run `txtodo device remove` by hand on a live group yet.
- Sync data still seals under a constant epoch 0 (`lan_session.rs:18`, `file_carrier.rs:64`), so
  a rotation does not cut the removed device off from new ops.
- A removed device is not refused: LAN resync (`lan_peers.rs` `peers_to_resync`) skips parked
  peers only, and incoming sessions do not look at `removed_at`. Verify before fixing.
- These overlap `tasks/sync-device-remove/todo.txt`'s open lines (stop accepting the removed
  device; an offline device finds its grant; old epochs still open). This backlog takes them over.

## Pruned device

- Its frames fail on every peer, so it parks them all. It says "dropped from the group; pair
  again", not just offline. Decided 2026-10-18 (human).

## Protocol design (agent, 2026-10-25), after reading the code

What the code does today, which the plan above did not spell out:

- The crypto parts exist and are epoch-aware: `seal`/`open` carry the epoch in the clear header and
  `GroupKeys` retains up to 16, `wrap_grant_for`/`open_grant`/`plan_rotation` wrap one key to one
  device's X25519 key. The daemon never uses them past epoch 0: `lan_session.rs`
  (`GROUP_EPOCH`, `fetch_group_key`, `single_epoch_keys`), `lan_session_shared.rs::send_message`,
  `file_carrier.rs` all take epoch 0; only the control channel already seals under
  `identity.group_epoch()` and opens only that one epoch (`control_session.rs`).
- **Pairing hands out epoch 0 forever.** `pairing_group_key.rs` reads `KeyId::Group(0)`, and the
  joiner stores what it is given at epoch 0 (`pairing_state.rs::commit_group_key`). After a rotation
  a re-paired device would get the *old* key (which a pruned device may still hold) and file it as
  the current one. So re-pairing a pruned device cannot work, and would be unsafe, until the grant
  carries the epoch. Added below as its own line.

Delta, in the order it can land with every step green:

1. **Epoch helpers (daemon).** `current key`: `KeyId::Group(identity.group_epoch())`. `retained
   keys`: epochs `cur-15..=cur` that the keystore holds, so a frame sealed before a rotation still
   opens. Sync sessions, the file carrier and the control channel seal under the current epoch and
   open under any retained one. With no rotation (epoch 0) nothing changes on the wire.
2. **Pairing carries the epoch.** `PairingGrant` gains the epoch; the initiator hands over the
   current key; the joiner stores it at that epoch and sets its own `group_key_epoch`. Older epochs
   are not sent: each hop seals under the sender's current epoch, so a new device needs only the
   current one.
3. **`ControlMessage::Grant`** (tag 4, `PROTOCOL_VERSION` 6): `{ sender, minter, epoch, wrapped
   (the `WrappedGrant`), left_out: devices the rotation dropped }`. And `control_epoch(frame)`, the
   epoch a control frame was sealed under, read from the clear header.
4. **Delivery needs no store.** The group key is in the keystore; a grant is `wrap_grant_for(key,
   epoch, peer_static_public)` and can be made again whenever it is needed. In a control session,
   the epoch a peer's first frame was sealed under *is* the epoch it holds. If it is below ours the
   sender sends the chain `peer+1..=cur`, each grant sealed under the epoch before it (the peer
   holds that one; the pruned device cannot read a grant anyway, it is wrapped to someone else's
   key). The receiver opens it with its static secret, stores the key at that epoch, advances its
   epoch, and drops every device in `left_out` ("dropped by <sender>"). Each device is told the
   same set, so they converge without a shared table.
5. **Same epoch twice.** Two devices that each prune mint an epoch N+1 key. A device records who
   minted the key it holds at each epoch; on a grant for an epoch it already holds with a
   different minter, the lower minter id wins and the other adopts it. The loser's own prunes are
   re-derived on its next tick (the peer is still stale, so it prunes again, to N+2).
6. **A removed device is refused** in the three places a peer is let in: LAN/relay resync skips it,
   the link `Hello` is refused, and a control session ignores its offers.
7. **Stale verdict and the prune loop** (lines below) call the same `Workspace::remove_device`.
8. **The pruned side.** Frames from a peer sealed under an epoch above ours that no grant has
   reached us for fail as `unknown_epoch`. Several in a row from every peer mean we were left out:
   "dropped from the group; pair again". (`wrong_group`, the parking signal, is a different group
   id and does not fire here.)

Not decided here, and why it can wait: what a removed device already holds. It keeps every op it
synced before (rotation covers new ops only), which `device remove` already says.
