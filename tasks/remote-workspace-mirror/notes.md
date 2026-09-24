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

## Design

Decided 2026-09-23 (human):

- Mirror root: reuse the per-OS data dir scheme (`txtodo-workspace-paths` /
  `default_workspace_dir(env)`, the same helper `ref:default-workspace` uses), e.g.
  `$XDG_DATA_HOME/txtodo/remote/<workspace-id>/` (macOS/Linux) and the `%LOCALAPPDATA%` equivalent
  on Windows.
- Trust model: auto-accept, no prompt. The peer is already paired/trusted, and mirrored content is
  non-executable data — same trust boundary sync already uses for every other op.
- Provenance flag: `WorkspaceInfo` needs a way to say "this root is a mirror I chose for you," not
  a path the user picked — surfaced as a "Remote" badge in the desktop switcher and a marker in
  `txtodo workspace list`/TUI, the same way `is_default` already marks the default workspace.
- `relocate`: once mirrored, a user may want the workspace at a real path (e.g. inside their actual
  project checkout). Needs to move the files and repoint the registry's root without touching the
  `WorkspaceId` or losing sync — same identity-preservation constraint `default-workspace`
  documented as unaddressed for its own directory.

## Adjusted 2026-09-24 (human)

- **Always separate, always in the data dir.** Every workspace synced from another device lands at
  `<data dir>/remote/<workspace-id>/` (sibling of `<data dir>/default/`) and shows as its own
  Remote entry in every workspace list. No path is ever user-chosen on the receiving side.
- Auto-accept is now unconditional, not "only when `local_dir` is omitted".
- So `workspace accept --dir`, the TUI accept pane's directory prompt, and
  `WorkspaceAcceptOfferRequest.local_dir` go (reserve the proto field, don't reuse the number).
- `relocate` is dropped: it existed only to move a mirror onto a user path, which "always" rules out.
- Exception, decided 2026-09-24 (human, option A): the *default* workspace keeps merging by ADR
  0029's reserved id — it is the one list shared across your devices, never a Remote entry. No
  ADR 0029 amendment needed. The foreign-device consent worry stays with
  `ref:default-workspace-pairing-consent`, which this does not settle.
- Unchanged: accept/decline RPCs stay (decline still means "don't mirror this one").

## Dependency

Needs `ref:workspace-offer-cli`'s `accept` command to exist first — this line's "no --dir" default
path is what makes that command usable without the user picking a directory by hand.

## Build plan (2026-09-24, agent)

- Provenance is derived, not stored: a root under `<state dir>/remote/` is a mirror. No registry
  column, no migration (same call `default-workspace` made for `is_default`).
  `WorkspaceInfo.is_remote` (field 11) carries it.
- Path helper: `txtodo_workspace_paths::remote_workspaces_dir_for(env)`, beside
  `default_workspace_dir_for`, so a hermetic test daemon never writes the real data dir.
- Auto-accept: the control channel still only records offers (it has no catalog). The offer
  registry wakes a mirror task (`tokio::sync::Notify`); the task, on a blocking thread, adopts each
  pending offer at `remote/<workspace-id>/` (dir + empty `todo.txt`), opens it so it is routed for
  sync, and consumes the offer.
- Skip rule: an id the registry has ever held (active or removed) is never mirrored. Active covers
  the default and a peer offering back our own workspace; removed means the user removed the
  mirror, which is how "don't mirror this one" sticks across restarts. A declined pending offer is
  remembered in memory so the next re-offer does not bring it back.
- Proto order: add `is_remote` first; reserve `local_dir` last, after every client stopped sending it.

## As built (2026-09-24)

- Commits: c8ec994 (proto is_remote), a438149 (paths), ec4c645 (daemon), 9d75e57 (cli),
  e47136b (tui), 65a4bbe (desktop), e98ff7b (proto reserves local_dir), then three one-line
  clean-ups.
- Daemon: `workspace_catalog_mirror.rs`. Mirror folder is `remote/` beside the default
  (`<state dir>/remote/` under `--dir`). `is_remote` is derived from the root, no column.
- Skip rule as planned: ever-registered ids are left alone; removing a mirror is the durable
  "not this one". Decline is in memory only.
- Clients: CLI `workspace list` shows `[remote]`; the TUI has no workspace list, so its status line
  says `remote workspace` when run inside a mirror; desktop switcher shows `Remote`.
- Still broken / not proven:
  - No two-daemon end-to-end test: offers travel the relay control channel only, and the relay
    tests need the public n0 relay. Unit tests cover the mirror; the wire path is untested here.
  - A declined offer comes back after a daemon restart and is mirrored then.
  - Nobody has looked at the desktop Remote label in the running app.
