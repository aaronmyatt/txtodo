# txtodo

A todo.sh-compatible [todo.txt](https://github.com/todotxt/todo.txt) tool with a
multi-device sync daemon, CRDT merge, and an MCP server for agents.

## Install

```bash
cargo build --workspace --release
# binary at target/release/txtodo
```

## Usage

```bash
txtodo [--dir DIR] [--json] [--no-id] [-A|--no-archive] [--no-daemon] <COMMAND>
```

- `--dir DIR` — todo directory for this run (overrides `$TXTODO_TODO_DIR` and config `todo_dir`).
- `--json` — one JSON object per line on listing commands.
- `--no-id` — don't stamp `id:` on added tasks (overrides config `id_tags`).
- `-A`, `--no-archive` — don't archive after `do`.
- `--no-daemon` — ignore a running daemon and edit the files directly.

### Task commands

| Command | Alias | Description |
|---|---|---|
| `add TEXT` | `a` | Add a task: today's date after the priority, then an `id:` tag. |
| `addm TEXT` | | Add several tasks, one per line of TEXT. |
| `append ITEM# TEXT` | `app` | Add text to the end of a task. |
| `prepend ITEM# TEXT` | `prep` | Add text after a task's priority and date. |
| `replace ITEM# TEXT` | | Replace a task's text, keeping its priority and date. |
| `del ITEM# [TERM]` | `rm` | Delete a task, or remove TERM from it. |
| `do ITEM#...` | | Mark tasks done and archive. |
| `pri ITEM# PRIORITY...` | `p` | Set task priorities (A-Z), as `ITEM# PRIORITY` pairs. |
| `depri ITEM#...` | `dp` | Remove a task's priority. |
| `move ITEM# DEST [SRC]` | `mv` | Move a task to another file in the todo directory. |
| `archive` | | Move completed lines to the bottom of `todo.txt`, drop blank lines. |
| `deduplicate` | | Blank every later repeat of an identical line. |
| `fmt` | | Canonicalise quirks in `todo.txt`. |
| `lint` | | Report quirks and file hygiene in `todo.txt`. |
| `report` | | Archive, then record task/done counts in `report.txt`. |

### Listing commands

| Command | Alias | Description |
|---|---|---|
| `list [TERM...]` | `ls` | List tasks matching every TERM (`-term` excludes), sorted. |
| `listall [TERM...]` | `lsa` | List every task in `todo.txt`, done tasks included. |
| `listpri [PRI\|A-C] [TERM...]` | `lsp` | List tasks with a priority, optionally filtered to one or a range. |
| `listproj [TERM...]` | `lsprj` | List the `+project` tags of matching tasks. |
| `listcon [TERM...]` | `lsc` | List the `@context` tags of matching tasks. |
| `listfile [FILE] [TERM...]` | `lf` | List the `.txt` files in the todo directory, or the tasks in FILE. |

### Daemon-mode commands

These need a running `txtodo daemon` for this workspace.

| Command | Description |
|---|---|
| `log [--file FILE] [-n N]` | Show the op log, newest first. |
| `blame ITEM#` | Who last touched each field of a task. |
| `undo [--steps N]` | Undo the newest N ops (default 1). |
| `checkout AT [--stdout] [--file FILE]` | Render a document as it was at a local date-time. |
| `conflicts [list\|resolve]` | Open `needs_review` flags and resolve them. |
| `pair [CODE]` | Pair with another device (no CODE starts a handshake; CODE joins it). |
| `open ITEM#` | Print the resolved `ref:` directory for a line. |
| `notes ITEM#` | Open `$EDITOR` on a line's `ref:`/`notes.md`. |
| `sub ITEM# CMD...` | Run CMD with its directory scoped to a line's `ref:` sub-list. |
| `prune --orphans [--yes]` | List `ref:` directories no line points to; delete only with `--yes`. |

### Service and diagnostics

| Command | Description |
|---|---|
| `daemon <start\|stop\|status\|install\|uninstall> [--force]` | Manage the `txtodod` service for this workspace. |
| `mcp [--stdio] [--http] [--lan] [--token TOKEN]` | Serve the Model Context Protocol surface for this workspace. |
| `doctor [--verbose]` | Check socket, watcher, files, clock and config; exit 1 on any failure. |
| `env` | Print the resolved paths and config. |

## Configuration

Resolved in this order: `--dir` > `$TXTODO_TODO_DIR` > config `todo_dir` > current directory.

Config file: `$TXTODO_CONFIG`, else `$XDG_CONFIG_HOME`/`%APPDATA%`/`~/.config`, then
`txtodo/config.toml`:

```toml
todo_dir = "~/todo"
identity_mode = "tagged"  # or "sidecar" (default)
url_schemes = ["http", "https"]
```
