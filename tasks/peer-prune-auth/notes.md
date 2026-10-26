# Peer-prune: removal holds only against an honest peer

## Goal
- A device the group removed can neither read new data nor write. Today it can do both.
- Found in a review of the peer-prune commits (97b8a808..b0df08c8), 2026-10-26.

## Findings
- Forged Grant (critical). `group_rotation.rs:133-138`: a Grant's `sender` and `minter` are what
  the message says; the only checks are "sender row known and not removed" and "frame opens under
  a retained epoch" (16 back). `wrap_grant_for` is ephemeral-static ECDH with no sender auth, and
  nothing ties the iroh node id to a `DeviceId` on accept. A pruned device holding K_N seals, under
  N, `Grant{sender: A, epoch: N+1, wrapped: its own key, left_out: [V's peers]}`; V adopts it,
  tombstones its real peers and seals new data under a key the removed device holds.
- Removed device still writes (high). `lan_session_dispatch.rs:122` refuses `Hello` by the
  `device` field the peer sends; ops sealed under any retained epoch verify with the group-derived
  key it still holds. `Hello{device: A}` under N is greeted and its ops applied.
- Tie-break undoes a manual remove (high). `group_rotation.rs:171-180`: the loser takes the
  winner's `epoch_info` and drops its own `left_out`; its target is already tombstoned, so
  `prune_stale::verdict` never prunes it again. Contradicts ADR 0044 §5 ("sends its prunes
  again"). `two_devices_that_rotate_at_once_end_on_one_key` never checks the ghost stays out.
- A live peer on an older protocol is pruned after 7 days (medium): `last_seen` moves only when a
  `Hello` opens; the verdict ignores `other_protocol`. After upgrading it must pair again.
- Anyone can raise `dropped_from_group` (low): `peer_keys.rs:258-275` counts unknown-epoch misses
  from any sender, read from the clear header before the AEAD check.
- No test for the control-channel refusal of a removed device (`control_flow.rs:118`).

## Open question (@human)
- Fix direction, ADR-sized: bind `DeviceId` to the iroh node id on accept; sign grants with the
  minter's static key; refuse frames under any epoch at or below the one the sender was dropped at.
