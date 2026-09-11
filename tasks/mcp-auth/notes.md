# MCP auth: bearer and stdio token (plan M6, plan §6)

Goal. Every MCP request is authenticated: bearer on HTTP, inherited token on stdio
(design §6.1/§6.2).

## HTTP
- Parse `Authorization: Bearer <token>` on every request; missing/malformed → 401.
- Verify via `txtodo-mcp::token::verify` with the daemon's root secret + revocation list.
- Resolve the caveats to a `Scope` and quarantine context; attach to the request so every
  handler and mutation carries it.

## stdio
- Token from `--token`; else the config's `default_stdio_token`.
- Add `default_stdio_token` to `config.toml` (`Config` in crates/txtodo-cli/src/config.rs),
  keeping the existing precedence rules (§2.2 rule 4: absent means default).

## Failures
- Distinct errors: no token, malformed token, expired, revoked, insufficient scope.
- Never log the token text (CLAUDE.md §3); log token_id only.

## Acceptance
- Valid bearer accepted; garbage/revoked/expired → 401.
- stdio without `--token` uses `default_stdio_token`; explicit `--token` wins.
