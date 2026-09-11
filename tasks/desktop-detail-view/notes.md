# Detail view: pinned parent, notes editor, recursive file view, breadcrumb, footer (plan M7, design §7)

## Goal

Double-click (desktop `Cmd/Ctrl+Enter`) on a line whose `ref:` directory exists opens the detail
view — plan §3.2 "Detail view", design §7 (clients): parent line pinned at top, `notes.md` as a
plain markdown editor, the sub-list rendered by the *same* file-view component recursively, a
breadcrumb, and a footer. The detail view is a page, not a modal (plan §3.3).

## Design

Everything the view shows comes from the daemon's tree (`ListFiles`) — the UI never resolves a
`ref:` slug or creates directories itself (design §2.6, plan §3.2 rule 2). The frontend is a thin
renderer over the Tauri command bridge from [desktop-tauri-shell](../desktop-tauri-shell/notes.md).

```ts
// apps/desktop/src/lib/types.ts — tree shape from daemon ListFiles (M5 adds progress)
export interface TreeFile {
  path: string;            // workspace-relative, e.g. "q4-roadmap/todo.txt"
  kind: "todo" | "done" | "notes";
  progress: { done: number; total: number } | null;   // plan §3.2.5; null for notes.md
}
export interface BreadcrumbStep { file: string; line: number; slug: string | null }
export interface DetailParams { file: string; line: number }   // workspace-relative file + line
```

```rust
// crates/txtodo-proto/proto/txtodo/v1/txtodo.proto — the two notes RPCs the view depends on (M5)
rpc GetNotes(TaskRef) returns (NotesDoc);       // notes.md bytes + Loro state for the line's ref:
rpc EditNotes(NotesEdit) returns (ApplyResponse);
// TaskRef { file: FilePath, task: TaskId } — daemon resolves task -> ref dir, never the client
```

```svelte
<!-- apps/desktop/src/lib/components/DetailView.svelte — layout skeleton -->
<header> <button on:click={back}>‹</button> <Breadcrumb steps={crumb}/> </header>
<section class="parent"><!-- pinned parent, highlighted, reuses <EditPopover/> --></section>
<section class="notes"><NotesEditor doc={notes}/></section>       <!-- CM6 markdown, no WYSIWYG -->
<section class="sublist">
  <h2>{progress.done} of {progress.total} done</h2>
  <FileView file={refFile} depth={depth + 1}/>                     <!-- recursive, own ref: lines -->
</section>
<footer>{absDir} · {syncStatus}</footer>
```

- **Breadcrumb** `todo.txt › line N`, nested `todo.txt › 2 › q4-roadmap/todo.txt › 3` (plan §3.2).
  Each step carries its file path + line; clicking a step pops that level.
- **Parent line** pinned, highlighted, reuses the single-click edit popover (plan §3.2). It is not
  re-rendered from the sub-list's file — it is the row from the *parent* file's `Watch` stream.
- **Notes editor** `<ref>/notes.md`: plain-text/markdown via CM6 `@codemirror/lang-markdown` with
  light highlighting (headings, list markers, code fences), no WYSIWYG (plan §3.2). Bound to
  `GetNotes`/`EditNotes`; keystrokes are `NotesEdit` ops (Loro text doc, M5).
- **Sub-list** `<ref>/todo.txt`: the same `FileView` component as the main view, fully recursive —
  lines click/double-click and carry their own `ref:` (design §7). Header `n of m done` from
  `TreeFile.progress`.
- **Footer**: absolute directory path + sync status (plan §3.2).
- **Lazy creation** (plan §3.2.4): both sections render when their files don't exist. The first
  keystroke into either triggers the daemon's M5 one-op-batch (add `ref:` tag + create directory);
  slug = kebab-case of the description, truncated to 40 chars, `-2`/`-3` on collision. The daemon
  renames the directory atomically + rewrites the tag in one op when the user renames the slug.

## Placement/dependencies

- New files under `apps/desktop/src/lib/components/` (`DetailView.svelte`, `Breadcrumb.svelte`,
  `NotesEditor.svelte`) + `src/lib/types.ts`; reuses `FileView.svelte` and `EditPopover.svelte`
  from the main-view task, and the `DaemonClient` from
  [desktop-tauri-shell](../desktop-tauri-shell/notes.md).
- Depends on `proto-grpc` (add `GetNotes`/`EditNotes` to the generated client),
  `daemon-service-files` (spawn), and M5's daemon tree model (`txtodo-model` progress, §3.2.5) —
  all already landed.
- No new crate, no new dependency beyond `@codemirror/lang-markdown` (needs the frontend
  `desktop-stack-mapping` sign-off + `cargo deny` for the Rust half).

## Edge cases & invariants

- **Dangling ref** (tag present, dir missing — §3.2.9): not an error; the view opens empty and
  lazy creation applies. `TreeFile` may be absent until the walker notices it.
- **Progress math** (§3.2.5): `done = completed lines in <ref>/todo.txt + task lines in <ref>/done.txt`;
  `total = task lines in <ref>/todo.txt + task lines in <ref>/done.txt`; blank lines excluded. The
  daemon computes it; the UI never recomputes.
- **Parent completion is never automatic** (§3.2.6): when `open == 0 && total > 0` the UI *offers*
  "Mark parent done"; the daemon does nothing on its own. Completing the parent does not touch the
  sub-list.
- **Archiving/deleting the parent** (§3.2.7, §3.2.10): keeps the `ref:` tag and leaves the
  directory alone — the detail view must not offer "delete directory".
- **Invariant (assert the negative)**: the Svelte bundle performs no slug resolution, no directory
  creation, no `fs` access — every read/write is a `DaemonClient`/Tauri command (design §7).

## Acceptance

- Double-click (or `Cmd/Ctrl+Enter`) opens detail; typing into empty notes creates the directory
  and adds the `ref:` tag in exactly one op batch; the parent file changes only on that one line
  (plan M7).
- Sub-list line double-click nests the breadcrumb to `todo.txt › 2 › q4-roadmap/todo.txt › 3`.
- Notes edits land as `NotesEdit` ops; sub-list edits as `Apply` ops; both sync to a fresh device
  (M5 acceptance).
- Progress header matches §3.2.5 for a fixture tree three levels deep including `done.txt`.

## References

- plan §3.2 (detail view + `ref:` convention), §3.3 (keyboard/a11y), M7 acceptance; design §7, §2.6.
- CM6 markdown: https://codemirror.net/docs/ref/#lang-markdown · Svelte 5: https://svelte.dev/docs
- [desktop-tauri-shell](../desktop-tauri-shell/notes.md) · M5 `ref:`/notes (txtodo-implementation-plan.md).
