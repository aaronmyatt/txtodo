# Agent principal and quarantine (plan M6, plan §6)

Goal. Every MCP mutation flows through the daemon's `Apply` as `Principal::Agent`, and the
quarantine caveat appends its context to added lines (design §6.2/§6.5).

## Principal
- The proto already carries `ApplyRequest.agent` (`AgentPrincipal { token_id, name }`);
  `daemon/src/convert.rs::parse_principal` maps it to
  `Principal::Agent { token_id, name, device }`. Fill `token_id`/`name` from the verified
  token, never from the client's word alone.
- No MCP path writes the file directly; everything is `Apply` through `handle.rs` (§6.1:
  the daemon is the only thing behind the surface).

## Quarantine
- `quarantine=@ctx` caveat appends the context to `todo_add` lines before `Apply`
  (default `@inbox`, per-token, §6.5). Add-only — complete/edit/delete are not re-tagged.
- The daemon stamps the creation date + `id:` as usual; quarantine adds one extra context.

## Attribution
- `txtodo blame` shows `agent:name@device` — the op log already stores the principal
  string (model `Principal::Agent` Display, op.rs).

## Acceptance
- Quarantined `todo_add` produces a line with the extra context (plan M6).
- `blame` on that line shows the agent principal, not `you@dev`.
