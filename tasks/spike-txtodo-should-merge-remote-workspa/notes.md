I am running into scenarios where I am working on a different device that is syncing the txtodo list from another device, but I am working on the same project. So when I kick off /txtodo-backlog from my coding agent it prioritises reading the current directories todo.txt, and ignores the remote txtodo list of the same name. 

How else can we synchronise this? Do we match by git remote? Some other unique identifier?

## Why it happens (2026-10-17, agent)

- A workspace id is minted on the device that first registers a folder. Two clones of one repo on
  two Macs get two ids (`daemon-workspace-identity-agreement`: independent ids never auto-merge).
- Mac A offers its `txtodo` (id X). Mac B mirrors it at `<data dir>/remote/X/` and shows it as
  Remote (`remote-workspace-mirror`, decided 2026-09-24: "always separate, always in the data dir").
- Mac B's clone is its own workspace (id Y), because it holds a `todo.txt`. An agent in the clone
  reads Y. X and Y never meet.
- `workspace rejoin` does not help: it swaps a folder's copy for a peer's under the *same* id.

## Options

- A. Match by git remote. The offer would carry a hash of the origin URL. Needs a field on the
  control message (postcard has no optional fields, so a protocol bump), and many workspaces are
  not repos. Good as a later, stronger hint; not the fix.
- B. Commit the workspace id to the repo (`txtodo.toml id = ...`), so clones share it. Both clones
  already hold the same lines under their own op ids, so the first sync lands every line twice
  (duplicates, ADR 0032). It also writes an identity into a user file.
- C. Union-merge X and Y. Same duplicate problem as B, for every line both copies hold.
- D. Redirect only: CLI/MCP in the clone talk to X, files stay where they are. The clone's files go
  stale, and agents read `notes.md` from disk.
- E. Bind: move the Remote mirror X into the clone folder. The clone's own copy (Y) moves aside into
  a backup folder (as `rejoin` does), Y is un-registered, X's registry row points at the folder.
  One folder, one list, one id. Explicit, because it moves files.

## Decided 2026-10-17 (agent)

E, with a name-match hint. Reasons:
- It is the only option with no duplicate lines and no stale files.
- It reuses `rejoin`'s tested move-aside code and its "never delete, never send" rule.
- It needs no wire change: offers already carry the name.
- Not automatic: two unrelated folders can share a name (`notes`), and binding moves the folder's
  files aside. The hint makes the right command one copy-paste away.

This reverses one line of the 2026-09-24 mirror decision ("no path is ever user-chosen on the
receiving side") for this case only: mirrors still land in the data dir; bind is the opt-in move.

## Design

- `txtodo workspace bind <remote-id>` (folder: cwd or `--dir`). Shows what moves and asks, unless
  `--yes`; `--dry-run` shows only.
- RPC `WorkspaceBind { workspace_id, root, dry_run }` → `{ workspace, backup_dir, moved,
  replaced_workspace_id }`. Additive proto, no protocol bump.
- Daemon `WorkspaceCatalog::bind`:
  1. X is registered and a Remote mirror; the folder exists, is not inside the data dir, and
     overlaps no registered workspace but Y (the one registered at that exact root, if any).
  2. Close X and Y quietly (rejoin's `close_quietly`).
  3. Move the folder's copy aside (`rejoin_backup::plan` + `move_aside`), check it is empty.
  4. Move the mirror's `.txtodo/` and documents into the folder (rename, same disk). On failure,
     move both back.
  5. Registry: remove Y, repoint X to the folder (`Registry::set_root`, new in txtodo-store).
  6. Open X at the folder. Sync carries on under X: same store, same op log, nothing re-sent.
- Hint: `txtodo doctor` warns when the current folder is a non-Remote workspace whose name equals
  a Remote workspace's name, and names the bind command.

## Known gaps (planned)

- The other Mac still holds a Remote mirror of Y (the clone's old id). Nothing offers Y any more;
  remove it there with `workspace remove`. Not automatic.
- A folder on a different disk from the data dir is refused (rename only, no copy).
- git and txtodo both carry `todo.txt` in a repo: a `git pull` can still trip over a synced edit.
  Not new; bind makes it reachable on the second Mac.

## As built (2026-10-17)

- store `Registry::set_root` (de4506bc); proto `WorkspaceBind` (f192c4e6); daemon
  `workspace_bind.rs` + `rejoin_backup::move_into` (0413a798); cli `workspace bind <id> [DIR]
  [--yes] [--dry-run]` (4856c97a); doctor `bind` row + playbook line (2ab63bda).
- The doctor row checks every own workspace against every mirror, not only the cwd's: the
  agent's cwd is not always the workspace root.
- `workspace list` does not mark the pair: doctor is the one place, and the playbook runs it.
- Proof: `tests/e2e/workspace_bind.rs`, two daemons. B binds A's mirror into its clone; A's
  next line reaches the clone, the clone's next line reaches A, no duplicates.
- Not tested: the CLI command against a real daemon (unit tests cover its text); a rename that
  fails half way across disks on Windows (no device check there; it moves back).
- Still open: the other device keeps its mirror of the clone's old id until removed by hand
  (`done_text` prints the command); not wired into the desktop or TUI (no "use here" button).
