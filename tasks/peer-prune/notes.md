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

### Control-session handshake (agent, 2026-10-25), found while designing grant delivery

Delivering a grant by "send it when we see the peer is behind" does not work as the control session
is written: both sides send their offers the moment the link opens, sealed under their current epoch.
A peer one epoch behind cannot open the first frame and ends the session before any grant can reach
it. So the session gets an order, and one more message (`ControlMessage::Epoch { sender, epoch }`,
tag 5, still protocol 6):

1. The **dialer** sends `Epoch(cur)` first, sealed under `min(cur, what it believes the peer holds)`,
   the belief being the peer's `devices.key_epoch`. The **acceptor** sends nothing until it has read
   a frame; it then answers `Epoch(cur)` sealed under `min(cur, the dialer's epoch)`.
2. On an `Epoch` from a peer: record that it holds that epoch (`key_epoch = max`). If ours is higher,
   send the grant chain `peer+1..=cur`, each `Grant(k)` sealed under epoch `k-1` (the peer holds that
   one by then). Then send offers and the own-device list, sealed under our current epoch.
3. On a `Grant`: apply it (rules in the design above), then answer `Epoch(cur)` as the ack.
4. `devices.key_epoch` now means *confirmed*: a rotation no longer sets it for the kept peers; only
   hearing a peer's `Epoch`, pairing, or a grant it acked does. (`device_remove.rs` set it at mint
   time and said it was not confirmation.)
5. Per epoch, `meta` keeps `group_epoch_info:<n>` = who minted the key and who the rotation dropped,
   so a grant can be made again for a peer that comes back later, and the tie-break has a minter.

A removed device is refused at the first frame of a control session as well (its `sender`).

## As built (agent, 2026-10-26)

Everything the design asked for is in, bar the one line only a person can do (the two-Mac check). The
lab scenario `prune` passes (three devices, a1 the hub).

- **Verdict** `prune_stale.rs`: parked and silent `prune_parked_after` (1 day), or silent
  `prune_quiet_after` (7 days); `0` turns a rule off. Silence counts from the newest of `last_seen`,
  `paired_at` and `up_since`. **Loop** `prune_loop.rs`, one per device, on the resync tick, running
  `remove_device_because`. **Config** `prune_config.rs`: env, then the CLI's `config.toml` (flat keys
  beside `sync_dir`, not the `[sync]` table the ADR first named), then the defaults.
- **Two safety rules the design did not have**, both found while writing the loop:
  - `up_since` (`prune_presence.rs`): a device that was off or asleep drops nobody on waking. A tick
    more than ten minutes after the last (or half the shortest threshold, for the lab) moves it to now;
    it is saved in `meta`, so a restart after a stop counts too.
  - A peer with a session open is touched every tick. `last_seen` is set when a `Hello` opens and a
    session can stay up for days, so a connected peer that made no ops would have been dropped.
- **Why and who** `removal_why.rs`: `manual`, `pruned_quiet`, `pruned_parked`, `left_out` (dropped by
  another device and followed), with the device that did it. `DeviceList` carries
  `removed_reason`/`removed_by`/`removed_at_ms`. Before this a row said only `removed`.
- **The pruned side** `Health.dropped_from_group`: three `unknown_epoch` opens in a row from anyone, ended
  by an opened frame, a new epoch or a new group. Not "every peer" as the design said: a dropped device
  often cannot tell who sent a frame it cannot open, so the count is not per peer.
- **Decision reversed:** the first cut left a removed device out of the dial set. Then a dropped device
  with the higher id (the one that does not dial) was never told. A removed device is dialed again, with
  the growing backoff of any dial that gets no `Hello` back, and its `Hello` is still refused. The relay
  paths still skip it; a relay device dials the pruner itself, through its control channel.
- **Clients:** `txtodo device list` (reason, `removed_at`), `doctor` (a `group` FAIL only when dropped,
  an ok `peer` row per dropped device), TUI Settings › Devices, desktop Settings › Devices (a note per
  dropped row, a banner). The `peer` row is ok, not warn: a retired device would otherwise warn forever and
  nothing can acknowledge it. Looks of the TUI and desktop are not checked.
- **Found by running it, not by reading it** (the two-daemon e2e and the lab each found something the
  unit tests did not):
  - Dial backoff against a removed device grows to a minute and nothing reset it, so a device paired
    again waited that out (`LivePeers::dial_generation`: a pairing, a new group or a new epoch makes every
    dial loop forget its backoff; the e2e fails with a long apart phase when this is off).
  - A dropped device woke, heard nothing it could open, read every peer as quiet and pruned them, minting
    a key of its own that nobody else held. A device that is `dropped_from_group` now prunes nobody.
  - A dropped device is dialed only now and then, so it may see one or two `unknown_epoch` opens, not three:
    a run of any length that has lasted two resync rounds counts as a drop too.
- **Tests:** unit tests per piece; `tests/e2e/prune_repair.rs` runs two real daemons through freeze,
  prune, apart, the dropped flag, pairing again and catching up (38 s, left out of the default nextest
  set). Lab scenario `prune` runs three devices.

Options weighed, and the pick:
- Prune clock: wall time from `last_seen` alone (rejected: a closed laptop drops everyone on waking) or
  with `up_since` (taken).
- Telling a dropped device: have it infer from failures (taken) or a new control message sent under the
  old epoch (rejected: a message that has to be sealed under a key the dropper is retiring, and a wire
  change for a case the failures already show).
- Config: daemon reads `config.toml` itself (taken: it starts from a service with no flags) or the CLI
  passes it (rejected: nothing threads config to the daemon today, see `key_store`).

Known gaps:
- A file-carrier-only device has no control channel, so it cannot receive a grant or hear it was dropped.
- When two devices prune at once the one with the higher id adopts the other's key and keeps its own
  drops until its clock prunes them again.
- The keystore keeps the lower-epoch keys of a group a device left. Harmless, unread.
- A pruned peer is pruned on each device by that device's own clock. Pairing with one device brings the
  data back through it (the lab: b1 pairs with a1 only, a2 gets b1's edits through a1), but a device that
  had paired with it directly keeps it listed as dropped until it pairs again. A device that never paired
  with it (a2 in the lab) has no row to mark; it just follows the new key.
- A device that follows a grant still opens frames sealed under the old epoch from a device it knows no
  row for, and does not refuse one the grant left out (its `left_out` list is not consulted at the
  `Hello`). The dropped device cannot open the answer, so nothing syncs, but it is a door left ajar.

## The two-Mac check (for the human, last open line)

Needs this build on both Macs (the installed `txtodod` predates it; protocol 6 refuses an older one).
1. On Mac A, set a short threshold so you do not wait a week: put `prune_quiet_after = "2m"` in
   `~/.config/txtodo/config.toml` (or run `txtodod` with `TXTODO_PRUNE_QUIET_AFTER=2m`) and restart A's
   daemon. Pair A and B if they are not paired.
2. Shut Mac B's lid, or stop its daemon, for over 2 minutes, plus a minute for A's session to end.
3. On A: `txtodo device list` shows B as `[pruned: not heard from for too long, removed_at=...]`. Add a
   line on A.
4. Wake B and add a line there. `txtodo doctor` on B has a `group FAIL dropped from the group`; the TUI
   (Settings › Devices) and the desktop app show the same. Neither line reaches the other Mac.
5. `txtodo pair` on A, `txtodo pair <code>` on B. Both lines should then be on both Macs.
6. Take the threshold line out of the config again.
