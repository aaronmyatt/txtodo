# Editor plugins (Neovim/VS Code/Obsidian): tokenize over the socket (plan M10, design §7)

## Goal

Three thin editor plugins — Neovim (Lua), VS Code (TypeScript), Obsidian (TypeScript) — that give
todo.txt files syntax highlighting, `+project`/`@context` completions, and inline conflict markers.
Design §7 editors row: "syntax highlighting generated from the same ABNF, completions for
`+project`/`@context`, inline conflict markers". Plan M10 (newer, wins): "editor plugins (Neovim, VS
Code, Obsidian) that use the LSP-style `tokenize` over the socket." None of the three parses the file
itself; the grammar lives in the core and is reached over the socket.

## Design

The seam is a new `Tokenizer` service in the shared proto
(`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`), implemented by `txtodo-daemon` with
`txtodo_core::tokenize` (declared in [../tui/notes.md](../tui/notes.md)):

```protobuf
service Tokenizer {
  rpc Tokenize(TokenizeRequest) returns (TokenizeResponse);   // "LSP-style": text in, spans out
  rpc Complete(CompleteRequest) returns (CompleteResponse);   // +project/@context candidates
}
message TokenizeRequest { string text = 1; }
message TokenizeResponse { repeated Span spans = 1; }
message Span { TokenKind kind = 1; uint32 start = 2; uint32 end = 3; }   // byte offsets, UTF-8 safe
enum TokenKind { COMPLETION_MARKER = 0; COMPLETION_DATE = 1; CREATION_DATE = 2; PRIORITY = 3;
  PROJECT = 4; CONTEXT = 5; TAG_KEY = 6; TAG_VALUE = 7; ID_TAG = 8; URL = 9; TEXT = 10; WHITESPACE = 11; }
message CompleteRequest { string file = 1; string prefix = 2; }
message CompleteResponse { repeated Candidate candidates = 1; }
message Candidate { string kind = 1;  /* "project" | "context" */ string value = 2; }
```

- The `Span`/`TokenKind` proto messages are a 1:1 projection of `txtodo_core::{Span, TokenKind}` so
  every client paints identical tokens (design §7).
- `Complete` is answered by the daemon's distinct project/context set (built from its projection
  index), filtered by `prefix`; the daemon owns the index, the plugins own nothing.
- Conflict markers: M4 flags concurrent same-word edits as `needs_review` and surfaces them via
  `Watch`'s `Change`. Each plugin renders a marker on flagged lines and offers
  mine/theirs/merged → `Apply { mutations: [Edit] }` (same resolve path as the TUI).

### Transport per stack (each plugin speaks the transport its runtime can)

- **Neovim (Lua)** — no gRPC-over-UDS in Lua: the plugin shells to `txtodo tokenize --json` /
  `txtodo complete --json` / `txtodo conflicts --json` (new CLI subcommands that proxy daemon-mode
  gRPC to the socket), debounced with `vim.schedule` + `uv.timer`. This is the "LSP-style tokenize"
  bridge: a request/response over the socket, not a full LSP server.
- **VS Code (TypeScript)** — a `language` extension whose activation listens to `todo.txt` files; it
  hits the JSON-over-HTTP mirror on loopback (ADR 0006: "generated from the same protobuf via
  tonic-web or a thin axum shim") with a generated client. Fallback when the mirror is down: spawn
  `txtodo tokenize --json`.
- **Obsidian (TypeScript)** — same mirror/fallback as VS Code; design §3 also assigns `wasm-bindgen`
  to Obsidian (`txtodo-ffi` WASM target), kept as the offline path so highlighting still works with no
  daemon. Socket-first per plan M10.

```typescript
// apps/editors/vscode/src/highlight.ts — the only highlighting entry point
export async function decorate(editor: TextEditor): Promise<DecorationSet> {
  const spans = await client.tokenize({ text: editor.document.getText() });  // TokenizeResponse
  return spansToDecorations(spans);   // Range(start, end) per span, kind -> theme token
}
export async function complete(prefix: string): Promise<CompletionItem[]>;   // +project/@context
```

```lua
-- apps/editors/nvim/lua/txtodo/highlight.lua
local function paint(buf)
  local text = table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n")
  local out  = vim.fn.system({ "txtodo", "tokenize", "--json", text })
  -- decode JSON spans -> nvim_buf_add_highlight per kind
end
vim.api.nvim_create_autocmd({ "BufRead", "TextChanged", "TextChangedI" }, {
  pattern = "todo.txt", callback = vim.schedule_wrap(debounce(paint, 150)) })
```

## Placement/dependencies

- New non-Rust area `apps/editors/{nvim,vscode,obsidian}/` — a second stack (Lua + TS), so it needs its
  own `stack.md` mapping (same requirement M7 raised for `apps/desktop`'s TS/Svelte). No
  workspace-crate boundary applies (none is a Cargo crate); `txtodo-cli` and `txtodo-daemon` do the
  Rust work.
- `txtodo-cli` gains `tokenize|complete|conflicts --json` (daemon-mode proxy) — allowed, it already
  depends on `txtodo-proto`. `txtodo-daemon` implements `Tokenizer` + `Complete` (it already depends
  on `txtodo-core`).
- No new workspace member → root `Cargo.toml` untouched. External deps for the daemon's mirror
  (`tonic-web` or an `axum` shim) need sign-off + `cargo deny check`.

## Edge cases & invariants

- No daemon: VS Code/Obsidian fall back to `txtodo tokenize --json` (direct-file mode still parses);
  Neovim degrades to no highlight with a one-line notice. Highlighting must never block the editor:
  cap the call and debounce (150 ms, matching the M3 watcher debounce).
- The editor's own buffer is the truth while editing; plugins only *decorate* and *complete* — they
  never mutate the file (the daemon does). Save goes through the normal reconciler path.
- Invariant: no plugin re-implements the grammar. Every span comes from `tokenize`; a hand-rolled
  regex highlighter is a boundary violation (design §7 "None of them parse the file themselves").

## Acceptance

- `txtodo tokenize --json` on a corpus line returns the same `Span`s as `txtodo_core::tokenize`
  (parity test against the M1 `.tokens.json` oracle).
- Neovim: open a todo.txt → buffer highlights match the §3.1 token colours; typing `+` offers existing
  projects, `@` offers contexts.
- VS Code: a todo.txt file shows decorations identical to the desktop app's CM6 tokens (shared
  `TokenKind`), and inline conflict markers appear when `needs_review` is set.
- Obsidian: works with the mirror up (socket) and with the daemon down (WASM fallback).
- Conflict marker → mine/theirs/merged resolves through `Apply` and clears the flag (shared test with
  the TUI's conflict pane).
- Each plugin ships a smoke test (headless nvim `--headless`, VS Code extension test runner, Obsidian
  unit test) that the decorate/complete path runs end-to-end against a temp-dir daemon.

## Frozen paths touched

- `.claude/stack.md` (add the Lua/TS stack mapping for `apps/editors/`) — frozen, ask.

## References

- plan M10 (txtodo-implementation-plan.md), design §7 + §3 (txtodo-design.md)
- protobuf: https://protobuf.dev/programming-guides/proto3/ · tonic-web: https://docs.rs/tonic-web
- Neovim Lua API: https://neovim.io/doc/user/lua.html
- VS Code language ext: https://code.visualstudio.com/api/language-extensions/overview
- Obsidian plugin: https://docs.obsidian.md · wasm-bindgen: https://rustwasm.github.io/wasm-bindgen/
- sibling: [../tui/notes.md](../tui/notes.md) (shared Tokenize/Complete/span proto)
