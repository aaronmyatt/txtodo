# txtodo-mcp

## Purpose
MCP tools, resources, prompts, capability tokens. Plan M6.

## Public interface
Tools per design §6.3, resources §6.4, `token` module.

## Invariants
- Every mutation goes through the daemon's Apply with Principal::Agent.
- Logs carry ids, counts and hashes — never line text, tokens or payloads.
- May depend only on: txtodo-proto, txtodo-query, txtodo-telemetry, txtodo-daemon-launch (matches
  `.claude/budgets.json`'s `allowedDeps.txtodo-mcp`). `main.rs` calls
  `txtodo_daemon_launch::ensure_daemon` before dialing the socket (task daemon-always-available);
  `grpc_backend.rs`/`global_socket.rs` stay pure and still don't import that crate.

## Task ids

A task's id comes from the daemon, not from the line: `GetFile` answers one id per line
(`FileContents.task_ids`, task `sidecar-task-ids`) and `doc.rs`'s `FileDoc` pairs them with the text.
Under Sidecar identity a line has no `id:` tag, so `TaskRow.id` and every id-addressed tool depend on
it. The first `id:` word in the text is only the fallback for a daemon that sent no ids.
`tests/sidecar_id_tools.rs` (`#[ignore]`d, needs a built `txtodod`) drives the tools under Sidecar.

## Tools MCP never mirrors

The registered tool set is a closed allow-list (`tests/smoke.rs`'s `EXPECTED_TOOLS`, an exact match).
These `txtodo` commands are deliberately not in it, and `no_tool_mirrors_a_trust_boundary_cli_command`
fails if a tool ever borrows their names: `workspace add|remove` (the device-global registry),
`pair` (cross-device pairing), `device remove` (group-key rotation, device trust), `identity migrate`
(rewrites every file of a workspace), `fmt` (rewrites lines it was not asked about; held until
agent-principal attribution covers a whole-file rewrite), and the local `bundle`, `daemon`,
`relay` and `skill` commands. "Mirror `txtodo-cli` as much as possible" stops at these.

