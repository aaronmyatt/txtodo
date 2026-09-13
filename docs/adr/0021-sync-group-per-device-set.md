# 0021 — One sync group per device-set, not one per workspace

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q8)

## Context
`Workspace` currently owns the device id, keystore and group key
(`crates/txtodo-daemon/src/workspace.rs:35-54`, `adopt_group_key` at :339); ADR 0010 puts state
under `<workspace>/.txtodo/`. As written (todo `daemon-workspace-actor`, line 132: "each with its
own store, op log, CRDT and sync Link"), pairing N workspaces would mean N separate `txtodo pair`
runs and N group keys in the keystore — "all your devices, all your projects" reads as pair once,
not once per project.

## Decision
We will pair once per device-set. One device identity and one sync group covers every workspace a
user has on a given pair of devices; a workspace becomes a namespace inside that group rather than
its own pairing/keystore boundary. Ops carry a `workspace_id` so a single `Link` multiplexes sync
traffic for every workspace between two devices, instead of one `Link` per workspace.

## Consequences
- Good: matches "universal" — pair a device once, every current and future workspace on it syncs;
  cheaper to build now than after M11's `WorkspaceActor` nesting lands.
- Bad: amends ADR 0010 — device identity and group key move out of `<workspace>/.txtodo/` to a
  device-set-scoped location; `Workspace` (`workspace.rs:35-54`) loses ownership of them, and every
  op gains a `workspace_id` it didn't carry before.
- Neutral / follow-ups: `daemon-workspace-registry` (todo 130) and `daemon-workspace-actor` (todo
  132) must be designed against a device-set-scoped group from the start. This ADR covers the
  sync-group-scope decision only; the broader global-daemon architecture (one `txtodod` per device,
  workspace registry) is tracked separately by todo `ref:adr-global-daemon`.

## Alternatives considered
- Per-workspace groups (status quo shape): simpler `Workspace` ownership model, but requires
  pairing once per project and multiplies group keys in the keystore for no benefit once a user has
  more than one workspace on the same device pair.
