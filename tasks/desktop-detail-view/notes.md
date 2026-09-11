# Detail view: pinned parent, notes editor, recursive file view, breadcrumb (plan M7, plan §3.2)

## Goal

Double-click (desktop `Cmd/Ctrl+Enter`) opens the line's `ref:` directory as a detail view per
plan §3.2 and design §2.6: pinned parent, notes editor, recursive sub-list file view, breadcrumb,
footer.

## Design

- Layout: header with back + breadcrumb `todo.txt › line N` (nested:
  `todo.txt › 2 › q4-roadmap/todo.txt › 3`). The parent line is pinned at the top, highlighted, and
  reuses the same click-to-edit popover.
- Notes section `<ref>/notes.md`: plain-text/markdown editor with light highlighting (headings,
  list markers, code fences), no WYSIWYG. CM6 markdown mode.
- Sub-list section `<ref>/todo.txt`: the *same* file-view component as the top level, fully
  recursive — its lines click, double-click, and carry their own `ref:`. Header shows `n of m done`
  (plan §3.2.5).
- Footer: absolute directory path and sync status.
- Both sections render even when their files don't exist yet. The first keystroke into either
  creates the directory and file lazily — the daemon's M5 one-op-batch ref creation, via
  `GetNotes`/`EditNotes` and the same `Apply` for the sub-list. The UI does not resolve the slug or
  create directories itself.
- Resolve the `ref:` slug to a sibling directory through the tree the daemon already walks
  (`ListFiles`); never re-implement slug resolution in the UI (design §2.6).

## Acceptance

- Double-click opens detail; typing into empty notes creates the directory.
- Sub-list line double-click nests the breadcrumb.
- Notes edits land via `NotesEdit` ops; sub-list edits via `Apply`; both sync like any document.

Refs: plan M7 and §3.2 (txtodo-implementation-plan.md), design §2.6 and §7 (txtodo-design.md).
