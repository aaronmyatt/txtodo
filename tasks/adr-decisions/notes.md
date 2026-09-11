# Write ADRs 0001–0012 from plan §1

Plan §1 is titled "Decisions already made — do not relitigate". Each row becomes one ADR with
Status **accepted**, Date 2026-09-11, Context = the row's rationale expanded to two sentences,
Decision = the row's text, Alternatives = the obvious one the rationale rules out.

| ADR | Plan row | Alternative to name |
|---|---|---|
| 0001 | Rust for everything below the UI | Go / TypeScript core with per-platform ports |
| 0002 | Loro | Automerge (move = delete+insert), Yrs |
| 0003 | iroh + mdns-sd | libp2p; hand-rolled QUIC + STUN |
| 0004 | SQLite via rusqlite, WAL | sled/redb; flat op files |
| 0005 | rmcp | hand-rolled JSON-RPC |
| 0006 | gRPC on the socket, REST mirror | REST only; JSON-RPC over socket |
| 0007 | Tauri 2 + Svelte 5 + CodeMirror 6 | Electron; native per OS |
| 0008 | UITextView / Compose BasicTextField + tokenize | web view on mobile |
| 0009 | id:<ULID> tag, tagged mode first | sidecar fingerprints first |
| 0010 | ports, names, paths (renamed to txtodo) | dynamic ports |
| 0011 | local date, YYYY-MM-DD, no TZ | ISO timestamps in the file |
| 0012 | ref: directory convention | richer line syntax; sidecar DB for notes |

## Rename note for 0010
Plan text still says `sisd`, `sis`, `$XDG_CONFIG_HOME/sisyphus/config.toml`, `<workspace>/.sisyphus/`,
`_sisyphus._udp`, `_sisyphus-mcp._tcp`. The ADR records the txtodo names (see rename-plan-names) and
notes the plan was updated in the same PR.

## Size
~25 lines each → ~300 lines total. Two commits of six.
