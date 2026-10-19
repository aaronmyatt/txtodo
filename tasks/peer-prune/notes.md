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
