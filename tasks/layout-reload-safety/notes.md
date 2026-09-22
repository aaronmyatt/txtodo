# layout-reload-safety

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`layout_reload` exists to refuse a layout change that would orphan live ref dirs. Three of its
error paths fail *open* instead, which is the one outcome the refusal was written to prevent.

## The gaps

- `crates/txtodo-daemon/src/layout_reload.rs:52-54` (and the `None => 0` at `:39`):
  `refs_in_the_old_place` returns `0` when `actor.ref_tags().await` errors — mailbox full, actor
  gone, store error. `apply` then sees `in_use == 0` and applies the new layout unconditionally.
  Live ref dirs stay in `tasks/` while the daemon starts resolving slugs to `elsewhere/`, silently
  orphaning every sub-backlog. The safe default on an unreadable root list is to refuse and leave a
  note.
- `layout_reload.rs:64-81`: `create_dir_all`, `fs::write` and `Workspace::register` are all
  `let _ =`. If the new `todo_file` path is an existing directory, or the write is denied, the
  layout is still set, `txtodo.toml` is still written, and `workspace_layout_impl`
  (`layout_rpc.rs:46`) returns OK — leaving a workspace whose root list has no actor, so every
  later `GetFile`/`Apply` fails with a confusing "no such document" and nothing says why. At
  minimum the RPC path should propagate; the watcher path can keep logging.
- `layout_reload.rs:31`: deleting `txtodo.toml` keeps the last good layout *and* clears the note
  (asserted by `layout_reload_tests.rs:98-105`), but `layout_file::initial` (`layout_file.rs:58-66`)
  falls back to `WorkspaceLayout::default()` on the next start. So a running daemon uses
  `refs_dir = "elsewhere"` while the on-disk truth says `tasks`, and the workspace silently changes
  shape at the next restart with no warning beforehand. The `Invalid` arm already leaves a note for
  `doctor`; `Missing`-after-valid should too.
- `crates/txtodo-daemon/src/tree.rs:134,157`: `rebuild_workspace_tree` calls
  `self.workspace().layout().get()` twice with several `.await`s in between. A hot reload landing in
  that window assigns node ids by the old layout while `build_with_layout` places ref dirs by the
  new one, producing a tree whose root-list children all appear as orphans until the next rebuild.
  Reuse the first binding.

`txtodo-model`'s `WorkspaceLayout` itself (the `refs_parent_of` / `ref_dir_for` split) is coherent
and well covered, including the "old list stops being the root list" case. The problem is only in
the daemon's reload plumbing around it.

See [[layout-client-gaps]].

## As built (2026-09-23)

All four closed in `layout_reload.rs`/`layout_rpc.rs`/`tree.rs`. The reload now fails closed on an
unreadable root list; the RPC refuses a `todo_file` it cannot create before anything is written and
reports a registration failure instead of returning OK; a deleted file under a non-default layout
leaves a `doctor` note; the tree reads the layout once. Unit tests in `layout_reload_tests.rs`.
