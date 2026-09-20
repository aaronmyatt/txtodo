# M6 security review: DRAFT for your review (2026-09-20)

Not signed off. Not in `RATCHET.md` yet: that file is append-only, so this becomes the entry only
after you sign. I read the code and existing tests. I did not run any new test except the slug fuzz
(row 6). "Verified" below means I read it in the code, with the file named.

## What changed since the M6 plan was written

- Bearer auth on MCP was never built, and on 2026-09-20 you decided it never will be: MCP is
  reachable from this device only (`tasks/mcp-local-only`). That removes `--lan`, so several M6
  lines below are now obsolete or change shape.
- The M6 notes talk about a macaroon root key. No such key exists in the code.

## The checklist, item by item

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | No secrets in logs | Pass by construction, no MCP test | Token code has zero logging calls (`txtodo-store/src/tokens.rs`, `txtodo-daemon/src/tokens.rs`). MCP tool calls use `#[tracing::instrument(skip_all, fields(tool, principal))]` (`txtodo-mcp/src/schema.rs`), so arguments are never logged and the principal is a non-secret id. M4 and M8 have capture tests; M6 has none. |
| 2 | Keys only in keystore | Pass, with a note | M4 tests cover key material. Token secrets are not keystore keys: 32 bytes of OS entropy, shown once, only `blake3(secret)` stored (`txtodo-store/src/tokens.rs`). There is no root key, so the "macaroon root key" audit does not apply. |
| 3 | Every network message versioned, authenticated, encrypted | **Fail for MCP HTTP** | Stdio has no network. HTTP is plaintext and has no authentication: `serve_http` mounts the service on a bare axum router with no auth layer (`txtodo-mcp/src/transport.rs`). Loopback only by default. See F1. |
| 4 | MCP HTTP refuses non-loopback unless `--lan` | Code does it, no test | Default bind is `127.0.0.1` (`MCP_LOOPBACK`). No test asserts it. `--lan` is itself the problem (F1). |
| 5 | Tokens never logged | Pass by construction, no test | Same as row 1. Also see F2: tokens are never checked. |
| 6 | Path traversal via `ref:` (fuzz the slug) | Pass at M4 and re-run | M4: 30 s run, one real crash, fixed (`tasks/security-m4-review/notes.md`). Re-run after M5's ref work: 60 s on 2026-09-20, 57.7 million runs, no crash (`just fuzz slug 60`, exit 0). Only the `slug` target ran, not `slug_windows_safe`. |
| 7 | Relay cannot tell op types apart | Closed in M8 | `RATCHET.md` 2026-09-14. Not an M6 item any more. |

## Findings that need your call

- **F1. `--lan` exposes an unauthenticated MCP on every interface. High while the flag exists.**
  - `--lan` binds `0.0.0.0:8636` (`MCP_LAN`). `main.rs` refuses `--lan` without `--token`, but only
    checks that a token is present. Nothing verifies it. Anyone on the network can call every
    tool, writes included.
  - Fix: line 1 of `tasks/mcp-local-only` (remove `--lan`). I'd do it first. Until then, do not
    run `--lan`.
- **F2. Tokens are minted but never checked.**
  - `Store::verify_token` has no caller. Scopes and revocation are not enforced anywhere.
  - With MCP local-only this is dormant, not a hole. It should be written down so nobody believes
    a scope restricts anything. Later choice: keep the token layer dormant, or remove it.
- **F3. Loopback is reachable by every local user and every browser tab.**
  - The `Host` and `Origin` check (line 2 of `tasks/mcp-local-only`) guards the browser case only.
    On a shared machine, other local users can still connect. Not solved.
- **F4. `payloadKB` has no honest number yet.** See the derivation below.
- **F5. Two tests are missing.** Loopback default (row 4), and no secrets in logs across token
  create plus a tool call (rows 1 and 5).

## payloadKB: derivation and proposal

`budgets.json` has `payloadKB: null`. It bounds one MCP/HTTP response. What bounds each shape today:

| response | cap today | worst case |
|---|---|---|
| `todo_list` rows | `limit` is client-chosen, "0/absent = daemon default" (`backend_args.rs`); the daemon's own ceiling is not verified | unbounded |
| history ops | `HISTORY_MAX_LIMIT = 1000`, summary 60 chars plus about 100 bytes fixed | about 160 KiB |
| whole-file resource | `MAX_LINES_PER_FILE = 1_000_000`, and no per-line cap (`MAX_LINE_BYTES` does not exist) | unbounded |

Proposal, each number an assumption for you to change:

- Pin `MAX_LINE_BYTES = 4096`. A line is one line of human text and the advisory hint is 100
  characters, so 4 KiB is generous.
- Cap `todo_list` at 50 rows per response. A row is the raw line plus its parsed fields; I assume at
  most twice the line, so 8 KiB worst case, 400 KiB for 50 rows.
- Page the whole-file resource at 512 KiB.
- Then `payloadKB = 512`. `budgets.json` and `stack.md` change together. `budgets.json` is a frozen
  path, and `.claude/UNFROZEN` exists, so I can edit it once you approve the number.

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
| Pin `MAX_LINE_BYTES` | Still needed |
| Derive and land `payloadKB` | Proposal above, needs your number |
| Update `stack.md`'s payload note | After the line above |
| Write the M6 findings in `RATCHET.md` | This draft, after you sign |
| Defer the relay op-type item to M8 | Already closed in M8 |
| Complete the parent line | After sign-off |

## To sign off

- [ ] F1 ordering accepted: remove `--lan` before anything else in `mcp-local-only`.
- [ ] `payloadKB = 512` with the three caps above, or your own numbers.
- [ ] The obsolete and changed lines in the table are rewritten as listed.
- [ ] This text goes into `RATCHET.md` as the M6 entry, with F2 and F3 recorded as accepted gaps.

M8 item 5 (re-run the inherited MCP-loopback, tokens-never-logged and slug-fuzz tests in the M8
job) stays blocked until the two missing tests in F5 exist.
