# ADR: global daemon — one txtodod per device with a workspace registry (plan M11)

Root todo.txt line 35: supersedes the per-directory daemon from plan M3/design §5. Design §5 always
described `txtodod` as one process per device; plan M3 and the actual build diverged into one
process per workspace directory. This task formalizes catching the implementation back up.

## As built (2026-09-13, orchestrator)

- `docs/adr/0025-global-daemon-one-process-per-device.md` — MADR-lite, per `tasks/adr-template`.
  Decision: one `txtodod` per device, a workspace registry, `WorkspaceActor` nesting per-workspace
  state (store/op log/CRDT) inside the one process, one socket per user with a workspace selector on
  every gRPC call, one service unit per device.
- Explicitly reconciled with two other same-day decisions so the twelve m11 tasks don't build against
  stale assumptions:
  - ADR 0021 (sync group per device-set): `WorkspaceActor` keeps its own store/op log/CRDT but does
    **not** own a private sync `Link` — sync is multiplexed once per device-set with a `workspace_id`
    on ops, at the registry's scope, not per-workspace. This narrows todo `daemon-workspace-actor`
    (38)'s original wording ("each with its own... sync Link") — read the ADR before building 38.
  - ADR 0024 (drop LAN transport, relay-only): `WorkspaceActor`'s sync side is transport-agnostic for
    now — M8's relay isn't built yet, so there is nothing real to wire a `Link` to yet regardless of
    ADR 0021's scoping. Don't block the registry/actor nesting work on relay existing.
- Not built here (by design — this task is the ADR only): the registry itself (`daemon-workspace-registry`,
  todo 36), the global socket (`daemon-global-socket`, todo 37), and the `WorkspaceActor` nesting
  (`daemon-workspace-actor`, todo 38) are separate, subsequent tasks that implement this decision.

## Judgement call, flagged for the human

Writing this ADR surfaced a real gap the twelve m11 todo lines don't yet cover: with LAN transport
dropped (ADR 0024) and M8's relay not yet built, there is currently **no working sync transport in
the codebase at all** for the target architecture — the LAN-based cross-device pairing handshake
just merged (`sync-pairing`'s LAN pass, commit `b099180`) is now dead code per ADR 0024's own
consequences section ("`sync-pairing`'s handoff... need re-scoping to the relay path only"). That
removal/re-scoping is real work, not yet a todo.txt line, and is out of this ADR task's scope —
flagged to the human as a separate follow-up rather than actioned unilaterally, since it would mean
reverting/reworking recently-verified, expensive work.
