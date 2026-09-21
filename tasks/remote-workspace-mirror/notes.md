# remote-workspace-mirror

## Goal

A workspace paired/offered from another device should be editable in every client (CLI, TUI,
desktop) even when this device has no matching filesystem path for it — mirrored into a
device-managed directory automatically, and visibly labelled "remote" rather than looking like a
normal user-chosen project folder.

## Why

User ask: `$HOME/Development/txtodo/todo.txt` on one machine may not exist on a paired machine, but
should still show up and be editable there. Today it doesn't, by design: the offer/accept protocol
(`tasks/daemon-workspace-identity-agreement/notes.md`) requires the accepting device to name its
own `local_dir` before anything is created (`WorkspaceAcceptOfferRequest.local_dir`,
`txtodo.proto:790-794`) — there is no auto-mirror and no "remote" concept anywhere in the registry
or the UI. Editing itself needs no new work once a workspace is registered (`Apply` is identical
for every workspace); the gap is entirely in discovery/registration and labelling.

## Precedent already in the codebase

`ref:default-workspace` solved the *single reserved* case of this same problem: one workspace
(the global default) syncs under a fixed `WorkspaceId` and lives "wherever that OS keeps txtodo's
data... the paths never need to match: devices agree on identity, not location"
(`tasks/default-workspace/notes.md`). `txtodo-workspace-paths`'s `default_workspace_dir(env)` is
the per-OS resolution helper to reuse/generalize. What that work explicitly left out: "Moving the
default's directory later is out of scope" — this line's `relocate` sub-task is that missing piece,
generalized to any mirrored (not just the one default) workspace.

## Design (open — needs the two @human decisions in todo.txt first)

- Mirror root candidate, following the default-workspace pattern: something like
  `$XDG_DATA_HOME/txtodo/remote/<workspace-id>/` (macOS/Linux) and the `%LOCALAPPDATA%` equivalent
  on Windows — but this is a new on-disk convention, not a bugfix, hence @human.
- Provenance flag: `WorkspaceInfo` needs a way to say "this root is a mirror I chose for you," not
  a path the user picked — surfaced as a "Remote" badge in the desktop switcher and a marker in
  `txtodo workspace list`/TUI, the same way `is_default` already marks the default workspace.
- Auto-accept trust model: does an incoming offer from an already-paired device mirror itself with
  no prompt (matches how sync already trusts a paired device for every other op), or does it still
  require an explicit `txtodo workspace accept`? This is a new trust boundary, not implied by
  anything already decided — @human.
- `relocate`: once mirrored, a user may want the workspace at a real path (e.g. inside their actual
  project checkout). Needs to move the files and repoint the registry's root without touching the
  `WorkspaceId` or losing sync — same identity-preservation constraint `default-workspace`
  documented as unaddressed for its own directory.

## Dependency

Needs `ref:workspace-offer-cli`'s `accept` command to exist first — this line's "no --dir" default
path is what makes that command usable without the user picking a directory by hand.
