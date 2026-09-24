# M6 security review: DRAFT for your review (refreshed 2026-09-24)

Not signed off. Not in `RATCHET.md` yet: that file is append-only, so this becomes the entry only
after you sign. First draft 2026-09-20; this refresh re-checks every row against the ~300 commits
since. "Verified" means I read it in the code, with the file named. Runs: the 2026-09-20 slug
fuzz (row 6), and the F5 and F6 tests (2026-09-24).

## What changed since the 2026-09-20 draft

- `mcp-local-only` shipped (37d7973, closed 2026-09-20). `--lan` is refused, `serve_http` has no
  address parameter, and the Host/Origin guard is in with tests. F1 is closed in code; F3 is half
  closed.
- `payloadKB` shipped: 512 in `.claude/budgets.json`, with the three caps that enforce it
  (0d88226, `tasks/payload-budget`, closed 2026-09-23). It is out of this review.
- New sync surface: `notes.md` (notes-sync) and `txtodo.toml` (199e849) now travel between paired
  devices. Rows 3 and 6 had to be re-checked for it, and row 6 found F6.
- Keystore work: a read cache (14bffbb), and the in-memory fallback now turns sync off with a
  debug-only test switch (8866667). Row 2 re-checked.
- Trust between paired devices changed: remote workspaces auto-accept (decided 2026-09-23), and
  `default-workspace-pairing-consent` is an open `@human` line. Not an M6 item; see F7.

## The checklist, item by item

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | No secrets in logs | Pass, tested | Token code has no logging calls (`txtodo-store/src/tokens.rs`, `txtodo-daemon/src/tokens.rs`). MCP tool calls use `#[tracing::instrument(skip_all, fields(tool, principal))]` (`txtodo-mcp/src/schema.rs`). `txtodo-mcp/tests/smoke.rs` (~line 320) captures a tool call's JSON log and asserts no token text. `txtodo-daemon/tests/tokens.rs` (`a_token_round_logs_no_secret_hash_or_caveat_value`) captures a create, list and revoke round and asserts no secret, `blake3` hex or caveat value (F5). |
| 2 | Keys only in keystore | Pass | Token secrets are not keystore keys: 32 bytes of OS entropy, shown once, only `blake3(secret)` stored (`txtodo-store/src/tokens.rs`). No macaroon root key exists. New: `keystore_cache.rs` holds `Secret` in memory; `Secret` wipes on drop and never prints its bytes (`txtodo-sync/src/keystore.rs`). The memory-keystore test switch is honoured only under `cfg!(debug_assertions)`, so a release daemon never reads it (8866667). |
| 3 | Every network message versioned, authenticated, encrypted | Pass | MCP: stdio has no network; HTTP is loopback-only with no address parameter (`txtodo-mcp/src/transport.rs`, `serve_http`). Peer sync: `notes.md` and `txtodo.toml` changes are ordinary `Op`s, grouped by file in `lan_apply.rs` like `todo.txt` ops, so they ride the same AEAD-sealed session (`txtodo-sync/src/aead.rs`). |
| 4 | MCP HTTP loopback only | Pass, tested | `serve_http(server, port, ct)` binds `SocketAddr::new(MCP_LOOPBACK, port)`; nothing can pass another address. `--lan` returns a refusal (`txtodo-mcp/src/main.rs`). `tests/http_loopback_bind.rs` starts the real server and dials it on 127.0.0.1 (served) and on this host's own address (refused) (F5). `tests/http_guard.rs` covers Host and Origin. |
| 5 | Tokens never logged | Pass, tested | Same as row 1. Also see F2: tokens are never checked. |
| 6 | Path traversal (slug, and now any peer-sent path) | Pass after the F6 fix | Slug: M4 30 s run, one crash, fixed; re-run 2026-09-20, 60 s, 57.7 million runs, no crash (`slug` target only). Synced `txtodo.toml`: goes through `layout_file::read`, and `WorkspaceLayout` rejects `..` and absolute paths (`txtodo-model/src/layout.rs`). Peer op file paths: validated on decode since 2026-09-24 (F6). |
| 7 | Relay cannot tell op types apart | Closed in M8 | `RATCHET.md` 2026-09-14; the padding scheme was then dropped by your decision 2026-09-17. Not an M6 item. |

## Findings

- **F1. `--lan` exposed an unauthenticated MCP on every interface. Closed.**
  - Fixed in 37d7973: `--lan` answers a plain refusal, `serve_http` takes a port only, mDNS is gone.
  - Still true: released builds v0.0.1 and v0.0.2 have the flag. Do not run them with `--lan`.
  - Regression guard: `tests/http_loopback_bind.rs` (bdbc331). Binding 0.0.0.0 fails it (checked
    by swapping the address). Known gap: it skips on a host with no network route.
- **F2. Tokens are minted but never checked. Unchanged.**
  - `Store::verify_token` is called only from `txtodo-store/tests/tokens.rs`. Scopes and
    revocation are enforced nowhere.
  - Dormant with MCP local-only, not a hole. Record it so nobody believes a scope restricts
    anything. Later choice: keep the token layer dormant, or remove it.
- **F3. Loopback is reachable by browser pages and other local users. Half closed.**
  - Browser pages: closed. rmcp's Host/Origin check is on; `tests/http_guard.rs` proves a foreign
    `Origin` and a rebound `Host` each get 403. Known gap: a browser MCP client on another local
    port (MCP Inspector) is refused too; there is no `--allow-origin`.
  - Other users on a shared machine: still open. Fine on a single-user laptop, not on a shared one.
- **F5. The two missing tests. Added 2026-09-24.**
  - Row 4: `txtodo-mcp/tests/http_loopback_bind.rs` (bdbc331); see F1.
  - Rows 1 and 5: `txtodo-daemon/tests/tokens.rs`, `a_token_round_logs_no_secret_hash_or_caveat_value`
    (b5d0889). Adding a `tracing::info!` of the secret in `token_create` fails it. Known gap: it
    drives the per-workspace `serve`, not the global service, so the `rpc_span` wrapper is not in
    the capture; that span carries only the method and workspace.
- **F6. A paired device could send an op whose file path escapes the workspace. Fixed
  2026-09-24.**
  - Fix: `FilePath` now deserializes through `FilePath::new` (`#[serde(try_from = "String")]`,
    `txtodo-model/src/ids.rs`). Serializing is unchanged, so signed ops' bytes and the
    `op_signing` golden do not move. Test: `file_path_decode_runs_the_same_checks_as_new` feeds
    postcard bytes for `../todo.txt`, `a/../../b`, `/etc/passwd`, `a\b`, empty and over-long
    paths; each fails to decode. `txtodo-model`, `txtodo-sync` and `txtodo-store` suites green.
  - Known gap: no daemon-level test sends a whole sealed frame with a bad path; the daemon suite
    was not re-run. The decode error surfaces as `MessageError::Codec` for the frame.
  - What it was, found by reading. `FilePath` checks its input only in `FilePath::new` (`txtodo-model/src/ids.rs`). It also
    derives `Deserialize` with no `#[serde(try_from)]`, so a postcard-decoded `Op` from a peer
    carries whatever path the peer wrote, `../` included.
  - Nothing re-checks it after decode. `lan_apply.rs` then runs `create_dir_all` on
    `root.join(path)`'s parent, and `Workspace::register` opens a file actor at `root.join(path)`
    (`txtodo-daemon/src/workspace.rs`). Notes and layout ops take the same route through
    `commit_notes_file`.
  - Who can do it: only a device holding the group key, since frames are sealed and ops signed.
    So this is a compromised or hostile paired device writing todo.txt-shaped text to any path the
    daemon's user can write, e.g. a shell rc file.
  - The `--sync-dir` file carrier decodes the same messages and gets the fix for free.
  - Weight: this matters more once remote workspaces auto-accept (F7).
- **F7. Trust between paired devices changed after the M6 checklist was written. Out of M6
  scope.**
  - A paired device will be able to add workspaces to this disk with no prompt
    (`remote-workspace-mirror`), and the default workspace merges on pairing with no accept step
    (`default-workspace-pairing-consent`, `@human`).
  - The checklist has no item for content a peer pushes. Add one to the next milestone review,
    not this one.
- (F4, the `payloadKB` number, shipped separately. See `tasks/payload-budget`.)

## The 12 lines in `tasks/security-m6-review/todo.txt`

| line | what happens to it |
|---|---|
| Review and sign off `findings-draft.md` | Widen to "F1 to F3, F5 and F6"; then sign |
| HTTP binds 127.0.0.1 by default; non-loopback errors unless `--lan` | Reworded and done 2026-09-24 (F5) |
| Tracing capture across token create, auth and a tool call | Reworded to create, list and revoke; done 2026-09-24 (F5) |
| Unauthenticated request is rejected even with `--lan` | Obsolete. Replaced by `tests/http_guard.rs` (shipped) |
| Re-check no secrets in logs on the new MCP paths | Done by reading (row 1) |
| Macaroon root key only via `KeyId::TokenRoot` | Obsolete: no root key. Row 2 records the token-secret facts instead |
| State the `--lan` threat model in `RATCHET.md` | Replace with the loopback threat model (F3) |
| Re-run the slug fuzz | Done for `slug` (row 6) |
| Write the M6 findings in `RATCHET.md` | This draft, after you sign |
| Defer the relay op-type item to M8 | Already closed in M8 |
| Complete the parent line | After sign-off |
| Peer op file paths are validated on decode (line 12) | Done 2026-09-24 (F6) |

## To sign off

- [ ] F1 is closed by 37d7973; the bound-address test is tracked as a regression guard.
- [ ] F3 is accepted as the residual risk: browser pages are closed, the shared-machine gap is
      accepted.
- [x] F6: fixed before M6 closes (your call, 2026-09-24).
- [ ] F7 goes to the next milestone review as a new checklist item.
- [ ] The line changes in the table are made.
- [ ] This text goes into `RATCHET.md` as the M6 entry, with F2 and F3 recorded as accepted gaps
      and `payloadKB` noted as shipped separately.

M8 item 5 (re-run the inherited MCP-loopback, tokens-never-logged and slug-fuzz tests in the M8
job) is unblocked: both F5 tests exist since 2026-09-24.
