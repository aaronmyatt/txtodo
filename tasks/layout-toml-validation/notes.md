# layout-toml-validation

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`<root>/txtodo.toml` has two independent readers that must agree, and they already do not. The
daemon's (`crates/txtodo-daemon/src/layout_file.rs`) validates; the CLI's
(`crates/txtodo-cli/src/config.rs:66-77`) does not.

## The hole

`root_list_name` accepts any string in `todo_file` and `resolve` joins it straight onto `dir`
(`config.rs:309-313`). So:

- `todo_file = "../../elsewhere/todo.txt"` makes `txtodo --no-daemon add` write outside the
  workspace.
- `todo_file = "notes.md"` makes the CLI edit a prose file the daemon explicitly refuses to treat
  as a list (`txtodo-model/src/layout.rs:143-149`).

The doc comment says "the daemon validates the file when it reads it" — true, and no protection at
all in direct-file mode, where no daemon is involved.

There is a second-order problem behind it: `resolve()` parses `txtodo.toml` itself rather than
asking the daemon, so it can hold a layout the daemon has refused. `layout_reload.rs:83-92` refuses
a hot reload while ref dirs still sit in the old place, and `layout_file.rs:58-67` falls back to
`todo.txt` on an invalid file. Hand-edit `txtodo.toml` to `todo_file = "work.txt"` in a workspace
with live ref dirs and the daemon keeps `todo.txt` while `txtodo add` targets `work.txt` — the CLI
creates and edits a document that is not the workspace's root list, and `txtodo list` shows nothing.

## Design

- In daemon mode, the root list comes from the `WorkspaceLayout` RPC (`commands/layout.rs:19-28`
  already calls it). The local parse is the direct-mode-only path.
- Direct mode must apply the same validation `WorkspaceLayout::new` does: relative, `/` separators,
  no `..`, no `:`, not under `.txtodo`, not a `notes.md`.
- Those rules currently live only in ADR 0030. `specs/ref-directories.md` is the mirrored spec both
  implementations are supposed to bind to — rule 2 there now says "the workspace's root list" but
  never defines what names it. Putting the validation rules in the spec is what stops the two
  parsers drifting again.
- `ABSTRACTIONS.md` is this repo's flag-don't-extract ledger and got no entry in 100 commits. Two
  independent `txtodo.toml` readers plus four clients each fetching the root list over the RPC is
  the same shape as the "five copies of the socket path" entry already at `ABSTRACTIONS.md:97`.

See [[layout-client-gaps]], [[layout-doc-drift]].
