# desktop-notes-hidden

Reported 2026-09-21: double-clicking a backlog item in the desktop app shows neither the nested
`todo.txt` nor the existing `notes.md`. Found by reading code only; nothing was run or watched in
the app, so treat both causes as hypotheses until the check below is done.

## Goal

Opening a task's detail view shows its `notes.md` text whenever the file exists, and its sub-list
whenever `<refs_dir>/<slug>/todo.txt` has tasks. A task with both shows both.

## Cause 1: the view shows notes OR the sub-list, never both (certain, by design)

`DetailView.svelte:282-306` renders the sub-list when `subListInfo.total > 0`, and the notes editor
only in the `{:else}` branch. The header comment (`DetailView.svelte:3-5`) says "exactly one of".
Every ref in this repo has both `todo.txt` and `notes.md`, so the notes never show for any of them,
whatever the daemon does. This is the design gap to close first (line 1).

## Cause 2: the installed daemon predates WorkspaceLayout (likely, unverified)

- `~/.cargo/bin/txtodod` was built 2026-09-20 20:58. `WorkspaceLayout` landed in `4ac8cad`
  (2026-09-20 21:57) and was reworked on 2026-09-21 (`ad3896a`).
- `GetNotes` resolves the path daemon-side (`crates/txtodo-daemon/src/notes.rs::ref_notes_path`), so
  an older daemon reads `<slug>/notes.md` beside the list, while the files sit in `tasks/<slug>/`.
  The daemon returns an empty doc without an error, so the editor is just empty.
- The desktop computes `tasks/<slug>` itself (`refDirFor`), so the two halves disagree and nothing
  says so. `MainView.svelte:80-88` swallows a failed `workspace_layout()`, which an old daemon would
  fail.
- Memory note: the installed daemon is the old build until it is reinstalled (see
  `tasks/sidecar-task-ids/notes.md`).

## Check before fixing (2 minutes, human)

1. Reinstall the daemon and `launchctl kickstart -k` the `com.txtodo.txtodod` job (line 2).
2. Reopen the app on this repo's workspace and double-click a line whose `tasks/<slug>/todo.txt` is
   empty or missing but which has a `notes.md`. If its notes now show, cause 2 is confirmed.
3. If nothing shows even then, the desktop is probably on the default workspace, not this repo.

## Design

- Notes go under the sub-list in one page, not a tab: a collapsible `Notes` section, open when
  `notes.md` has text, so a long sub-list does not push it out of sight.
- The footer shows `NotesDoc.path` from the daemon, so a client/daemon path mismatch is visible.
- Related, already on the backlog: `desktop-ref-indicator-path` (same layout drift at another call
  site, and the mock workspace layout disagreeing with the mock file tree). Do not duplicate it.

## Not decided

- Whether an old daemon should be refused outright (version handshake) rather than warned about.
  `version-info` already warns on app/daemon version mismatch; check why it did not fire here.
