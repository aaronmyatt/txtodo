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

## As built (2026-09-13, agent)

Nothing existed for this task yet (unlike most of its sibling desktop-* tasks) — built from
scratch this session, on top of the already-built `FileView`/`EditPopover`.

- `apps/desktop/src/lib/types.ts` — `DetailParams`/`BreadcrumbStep` (`{file, line}`), the
  navigation-stack shape; not a daemon DTO, so kept separate from `$lib/daemon.ts`.
- `apps/desktop/src/lib/components/Breadcrumb.svelte` — a leading "Home" crumb plus one
  file/line pair per open level, `onNavigate(stackLength)` truncates to that depth.
- `apps/desktop/src/lib/components/NotesEditor.svelte` — CM6 + `@codemirror/lang-markdown`
  (**new npm dependency** — this task's own notes pre-approved it: "no new dependency beyond
  `@codemirror/lang-markdown`"; still flagging for the human's `cargo deny`-equivalent sign-off
  since that's this repo's convention for every new dependency), debounced (500 ms) whole-document
  `editNotes` calls, flushing on unmount so a fast close doesn't drop the last keystrokes.
- `apps/desktop/src/lib/components/DetailView.svelte` — pinned parent (reuses `EditPopover`
  inline), the notes section, the recursive sub-list (`<FileView path={subListPath} depth={depth+1}
  onDetailRequest={onNavigateInto}>` — the *same* component, not a re-implementation), a "Mark
  parent done" offer gated on `done === total && total > 0` (never automatic), and a footer showing
  the absolute `ref:` directory (via a new tiny `workspace_root` Tauri command, since nothing
  exposed the workspace's absolute path before this).
- `apps/desktop/src/lib/components/{MainView,FileView}.svelte` — `MainView` now owns a `detail:
  DetailParams[]` stack (empty ⇒ root file view; non-empty ⇒ `DetailView`, replacing the screen,
  never overlaying it — plan §3.3 "a page, not a modal"); `FileView` gained `onDetailRequest` (see
  `desktop-main-view`'s own "As built" for that half).
- `apps/desktop/src-tauri/src/commands.rs` + `$lib/daemon.ts` — `getNotes`/`editNotes` wrappers
  around the *already-implemented* `GetNotes`/`EditNotes` Tauri commands (`commands_notes.rs`,
  `dto_notes.rs` — these existed before this session but had no frontend caller yet), plus the new
  `workspace_root` command/wrapper above.

### A real gap found, not invented by this session: `identity_mode` breaks notes for most tasks

While wiring the parent line's `task_id` (needed for `GetNotes`/`EditNotes`, which resolve by
`task_id` alone — `crates/txtodo-daemon/src/notes.rs::locate_task`, ignoring `line_number`
entirely), I found the repo's own recent `identity_mode` change (git log: "sidecar becomes the
default") means a fresh task's `id:` is **not written into the file** by default any more — so the
`/\bid:(\S+)/` regex this view (and `FileView`'s existing hover-pencil) use to get a `task_id`
returns empty for almost every task in a normal, unmodified workspace. Confirmed with two research
passes plus a hand-written daemon integration test
(`apps/desktop/src-tauri/tests/new_rpcs.rs::get_notes_and_edit_notes_lazily_create_the_ref_dir_through_the_bridge`,
added this session) that:

- `Apply`'s `Edit`/`Complete`/`Delete`/`Move` resolve by `line_number` and treat a blank
  `task_id` as harmless (`crates/txtodo-daemon/src/mutation.rs::resolve`) — **unaffected**, the
  pencil/save flow works fine regardless of identity mode.
- `GetNotes`/`EditNotes` resolve **only** by `task_id` — under sidecar mode, a task the desktop
  didn't itself just create (and whose creation op it happened to observe) has no
  client-discoverable `task_id` at all. No RPC exposes a per-line `task_id` for an arbitrary
  existing file today (checked `ListFiles`/`GetFile`/`FileInfo`/`FileContents` in the proto).
- A workspace with even one hand-written `id:` tag on disk auto-detects `identity_mode = tagged`
  for its whole lifetime (`crates/txtodo-daemon/src/workspace.rs`), which is why every Playwright
  fixture that needs a working notes editor (`apps/desktop/e2e/fixtures.ts`) seeds one.

`DetailView.svelte`'s notes section handles this honestly rather than papering over it: when the
parent line has no discoverable `task_id`, it shows an explanatory empty state ("Notes aren't
available for this task yet...") instead of mounting `NotesEditor` against an id that would just
fail server-side. I flagged the underlying gap as a follow-up task (spawn_task: "Expose per-line
task_id so desktop Notes work under sidecar mode") rather than inventing a workaround inside
`apps/desktop` — resolving it needs a proto/daemon-side decision (extend `ListFiles`/`GetFile`, or
add a dedicated resolve RPC), which is out of this task's scope and adjacent to an already-flagged
open question in this repo (`todo.txt`'s Q6 on `identity_mode` policy).

A second, narrower gap in the same area: the sub-list's own lazy creation ("first keystroke into
the *sub-list*, not notes, creates the `ref:` dir") is **not implemented server-side at all** —
`ensure_ref_dir` (the function that mints a `ref:` tag + directory) is called from exactly one
place in the whole daemon, `notes.rs`'s `EditNotes` handler; `Apply` on a not-yet-registered path
just 404s. `DetailView`'s sub-list section reflects this too: it shows an empty state rather than a
broken `FileView` when the directory doesn't exist yet. Typing into **notes** first still works and
is real (see the daemon-side test above); typing into an as-yet-nonexistent sub-list first does
not, today.

### Tests

- `apps/desktop/src-tauri/tests/new_rpcs.rs::get_notes_and_edit_notes_lazily_create_the_ref_dir_through_the_bridge`
  (new) — real daemon, real `Apply(Add)`, then `GetNotes`/`EditNotes`/`GetNotes` proving the lazy
  `ref:` + `notes.md` creation server-side path this view depends on.
- `apps/desktop/e2e/detail.spec.ts`, `notes-create.spec.ts`, `breadcrumb.spec.ts` (Playwright,
  DOM-structural, real daemon) — see `desktop-playwright-tests`'s "As built" for the harness.
- No new pure-logic unit test file: this task's only new "logic" (breadcrumb truncation, ref-dir
  path derivation) is either a one-line array slice or already covered by `lineInfo.test.ts`'s
  existing `findRefTag`/`dirOf`/`joinPath` coverage, reused as-is.

## What to open and look at

- `npm run tauri dev` in a workspace whose `todo.txt` has a hand-written `id:<ulid>` tag on one
  line and a `ref:<slug>` tag pointing at a real `<slug>/todo.txt` with a task or two in it.
  Double-click that line: expect the pinned parent at top, a working notes editor below it (type
  in it, close and reopen the detail view, confirm the text persisted), and the sub-list rendering
  `<slug>/todo.txt`'s own tasks with an `n of m done` header.
- Double-click a line in the sub-list: the breadcrumb should read
  `Home › todo.txt · 1 › <slug>/todo.txt · 1` (or whatever line numbers apply), and the pinned
  parent should now show the sub-list's own line. Click the `todo.txt` crumb: back to the
  first-level detail view; click "Home": back to the root file view.
- Double-click a line with **no** `id:` tag: the notes section should show the "Notes aren't
  available..." explanatory text, not a raw error or a silently-broken editor.
- Complete every sub-task so `done === total`: a "Mark parent done" button should appear above the
  sub-list; clicking it completes the parent line without touching the sub-list.
