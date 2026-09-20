# M6 security review: DRAFT for your review (2026-09-20)

Not signed off. Not in `RATCHET.md` yet: that file is append-only, so this becomes the entry only
after you sign. I read the code and existing tests. I did not run any new test except the slug fuzz
(row 6). "Verified" below means I read it in the code, with the file named.

## What changed since the M6 plan was written

- Bearer auth on MCP was never built, and on 2026-09-20 you decided it never will be: MCP is
  reachable from this device only (`tasks/mcp-local-only`). That removes `--lan`, so several M6
  lines below are now obsolete or change shape.
- The `payloadKB` number is split out of this review (2026-09-20). It bounds response size, not who
  can call, so it is not a security finding. It is tracked in `tasks/payload-budget`, low priority,
  and no longer holds up this sign-off.
- The M6 notes talk about a macaroon root key. No such key exists in the code.

## The checklist, item by item

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | No secrets in logs | Pass by construction, no MCP test | Token code has zero logging calls (`txtodo-store/src/tokens.rs`, `txtodo-daemon/src/tokens.rs`). MCP tool calls use `#[tracing::instrument(skip_all, fields(tool, principal))]` (`txtodo-mcp/src/schema.rs`), so arguments are never logged and the principal is a non-secret id. M4 and M8 have capture tests; M6 has none. |
| 2 | Keys only in keystore | Pass, with a note | M4 tests cover key material. Token secrets are not keystore keys: 32 bytes of OS entropy, shown once, only `blake3(secret)` stored (`txtodo-store/src/tokens.rs`). There is no root key, so the "macaroon root key" audit does not apply. |
| 3 | Every network message versioned, authenticated, encrypted | **Fail while `--lan` exists** | Stdio has no network. HTTP is plaintext and has no authentication by design: `serve_http` mounts the service on a bare axum router (`txtodo-mcp/src/transport.rs`). That is acceptable only if it is loopback-only, which is what `mcp-local-only` makes true. See F1. |
| 4 | MCP HTTP refuses non-loopback unless `--lan` | Code does it, no test | Default bind is `127.0.0.1` (`MCP_LOOPBACK`). No test asserts it. `--lan` is itself the problem (F1). |
| 5 | Tokens never logged | Pass by construction, no test | Same as row 1. Also see F2: tokens are never checked. |
| 6 | Path traversal via `ref:` (fuzz the slug) | Pass at M4 and re-run | M4: 30 s run, one real crash, fixed (`tasks/security-m4-review/notes.md`). Re-run after M5's ref work: 60 s on 2026-09-20, 57.7 million runs, no crash (`just fuzz slug 60`, exit 0). Only the `slug` target ran, not `slug_windows_safe`. |
| 7 | Relay cannot tell op types apart | Closed in M8 | `RATCHET.md` 2026-09-14. Not an M6 item any more. |

## Findings

- **F1. `--lan` exposes an unauthenticated MCP on every interface. Closed once `--lan` is removed;
  the code is pending.**
  - `--lan` binds `0.0.0.0:8636` (`MCP_LAN`). `main.rs` refuses `--lan` without `--token`, but only
    checks that a token is present. Nothing verifies it. Anyone on the network can call every
    tool, writes included.
  - The decision alone does not close it. Released builds (v0.0.1, v0.0.2) still have the flag.
    It closes when line 1 of `tasks/mcp-local-only` lands, plus a test that no flag or config can
    bind a non-loopback address. Until then, do not run `--lan`.
- **F2. Tokens are minted but never checked.**
  - `Store::verify_token` has no caller. Scopes and revocation are not enforced anywhere.
  - With MCP local-only this is dormant, not a hole. It should be written down so nobody believes
    a scope restricts anything. Later choice: keep the token layer dormant, or remove it.
- **F3. The main risk left after local-only: loopback is reachable by browser pages and by other
  local users.**
  - A hostile web page can rebind a hostname to `127.0.0.1` and call the tools as if it were local.
    The `Host` and `Origin` check (line 2 of `tasks/mcp-local-only`) is what stops that. The MCP
    transport spec asks local HTTP servers to validate `Origin` for this reason.
  - Other users of a shared machine can still connect. Not solved. On a single-user laptop this is
    an acceptable gap; on a shared machine it is not.
- **F5. Two tests are missing.** Loopback default (row 4), and no secrets in logs across token
  create plus a tool call (rows 1 and 5).
- (F4, the `payloadKB` number, was split out. See `tasks/payload-budget`.)

## The 13 lines in `tasks/security-m6-review/todo.txt`

| line | what happens to it |
|---|---|
| HTTP binds loopback by default; non-loopback errors unless `--lan` | Change: "no flag or config can bind a non-loopback address", after `mcp-local-only` |
| Tracing capture across token create, auth and a tool call | Keep, drop "auth" (none exists) |
| Unauthenticated request is rejected even with `--lan` | Obsolete. Replaced by the foreign-`Origin` test in `mcp-local-only` |
| Re-check no secrets in logs on the new MCP paths | Done by reading (row 1); the capture test makes it real |
| Macaroon root key only via `KeyId::TokenRoot` | Obsolete: no root key. Replace with the token-secret facts in row 2 |
| State the `--lan` threat model in `RATCHET.md` | Replace with the loopback threat model (F3) |
| Re-run the slug fuzz | Done for `slug` (row 6) |
| Pin `MAX_LINE_BYTES`; derive `payloadKB`; update `stack.md`'s payload note | Moved to `tasks/payload-budget` |
| Write the M6 findings in `RATCHET.md` | This draft, after you sign |
| Defer the relay op-type item to M8 | Already closed in M8 |
| Complete the parent line | After sign-off |

## To sign off

- [ ] F1 closes with line 1 of `mcp-local-only` and the no-non-loopback-bind test.
- [ ] F3 is accepted as the residual risk: the Host/Origin check ships, and the shared-machine gap
      is accepted.
- [ ] The obsolete and changed lines in the table are rewritten as listed.
- [ ] This text goes into `RATCHET.md` as the M6 entry, with F2 and F3 recorded as accepted gaps
      and `payloadKB` noted as tracked separately.

M8 item 5 (re-run the inherited MCP-loopback, tokens-never-logged and slug-fuzz tests in the M8
job) stays blocked until the two missing tests in F5 exist.
