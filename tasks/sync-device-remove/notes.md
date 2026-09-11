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
