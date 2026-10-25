# 0044 — A device drops peers it has not heard from, by itself

- Status: accepted 2026-10-25 (the options below were decided by the owner on 2026-10-17 and
  2026-10-18; the thresholds and the two rules marked "agent pick" are the agent's, to be revised in
  the open). Plan in `tasks/peer-prune/notes.md`.
- Date: 2026-10-25
- Deciders: owner (option A, per-device pruning, the pruned device's message); agent (the rest)

## Context
A paired device that goes away for good stays in every other device's `devices` table. On
2026-10-17 one peer (paired 2026-10-03, last seen that day) held `SyncStatus.pending_ops` at 3930 and
growing. It holds no key we share, so it could never sync again without a new pairing, and nothing
ever removed it. `txtodo device remove` already tombstones a row and rotates the group key, but a
person has to run it, and the rotation is only half built: the grants are minted and never
delivered, sync data still seals under a constant epoch 0, and a removed device is not refused.

## Decision
1. **A stale peer is dropped by the daemon, through the same `device remove`.** The row is
   tombstoned and the group key rotates. The owner chose this over a saved, undoable "retired"
   mark with no rotation: re-pairing is always there, and a dead peer should stop being a
   recipient of new ops early. A pruned device that returns says "dropped from the group; pair
   again" and pairs again; the row comes back (`removed_at = NULL`), so nothing else is needed.
2. **Stale** is a verdict from this device's own `last_seen` for the peer, which is on disk:
   **parked** (three `wrong_group` opens in a row, `peer_keys.rs`) and not seen for
   `prune_parked_after`, or **quiet** (keys match, not heard from) for `prune_quiet_after`.
   Agent pick: 1 day parked, 7 days quiet. Config `[sync] prune_parked_after`,
   `prune_quiet_after`; `0` turns that rule off. Never this device itself; the last peer may go.
3. **Every device prunes on its own.** Each keeps its own `devices` table and decides from its own
   `last_seen`. There is no election and no shared list.
4. **A receiver follows the grant.** The group key is shared, so one device's prune rotates it for
   everyone, and the pruned peer gets no grant: it cannot read new ops from any device, even one
   that still hears it. A device that receives a grant marks every peer the grant leaves out as
   dropped here too ("dropped by `<device>`"). Agent pick. This is the cost of decision 3: a device
   that cannot reach a peer others can reach still cuts it off from everyone, so the thresholds are
   long.
5. **Two devices pruning at once mint two epoch N+1 keys.** Agent pick: the grant from the lower
   device id wins; the other adopts it and sends its prunes again.
6. **Ops seal under the current epoch, and old epochs still open.** The sync stream and the file
   carrier stop sealing under the constant epoch 0, and `GroupKeys` keeps the retained epochs
   (`MAX_RETAINED_KEY_EPOCHS`), so ops sealed before a rotation still open.
7. **A removed device is refused.** Live sessions and LAN resync skip it and its incoming `Hello`
   is refused, so a prune stops it at once, not at the next epoch.

## Consequences
- A peer that is merely away for a week is dropped and must pair again. History already synced
  stays on its disk; a rotation covers new ops only.
- A flaky link between two devices can cut a third device off from the whole group (decision 4).
  Long thresholds make this rare; it is the price of having no election.
- The wire grows a message that carries grants (`PROTOCOL_VERSION` moves), and sealing under the
  current epoch changes what a frame's epoch field holds. A device on the old protocol is refused
  both ways until upgraded, as for any protocol bump.
- `txtodo device list`, `doctor`, the TUI and the desktop show pruned rows and why.

## Alternatives considered
- **A saved, undoable "retired" mark, no rotation (owner's B).** Keeps the peer's key valid, which
  is what a dead peer must not have.
- **Per-sender keys (each device rotates only its own sending key).** Keeps a prune truly local but
  rebuilds the crypto.
- **One elected device prunes for the group.** Needs an election and a shared device list, which
  the devices tables do not have (ADR 0029's `OwnDevices` only says which peers are own).
- **Prune only when a human confirms.** The owner wants peers dropped early.
