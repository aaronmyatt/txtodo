# two-kinds-of-workspace

## Goal

User stories (2026-10-25, human):

- As a user I want any number of named lists like the default, synced across my devices, so my
  project-agnostic todos stay in sync.
- As a developer I want a repo's todo.txt and tasks/ synced by git, so they match my workflow and I
  never lose content in the flow.

## Why (checked on one Mac, 2026-10-25)

- An MCP edit in a repo did not reach the checkout. The checkout had no registry row, so MCP sent
  the write to `default/` and said so only on stderr, which agent hosts hide
  (`txtodo-mcp/src/main.rs` `aim_at_the_current_workspace`).
- The real synced txtodo list sat in `<data dir>/remote/01M3ZK…/`, a mirror the checkout never saw.
- `givens` was there twice: the clone and `remote/01M4MJ…/`, 12 lines apart.
- A clone on a second device mints its own id, so it never merges with the first device's list.
  `workspace bind` (2026-10-17) fixes that by hand, one repo at a time.
- git already carries these files. Syncing them a second way gives two truths.

## Design

Decided by the agent 2026-10-25 on the human's brief; change any line.

1. **Two kinds, set by where the folder is. No new setting.**
   - List: in the data dir (`<data dir>/lists/<name>/`, plus `default/`). txtodo syncs it.
   - Repo: any other folder, usually a git checkout. txtodo never syncs it; git does.
   - Rejected: a per-workspace `sync = git|txtodo` flag. One more knob, and "both" is the bug.
2. **A list's id comes from its name.** `blake3("txtodo-list:" + lowercased name)`, cut to a
   `WorkspaceId` with a fixed marker so it never equals a minted ULID or the default's reserved id.
   - Two devices that both create `work` merge with no offer, as the default does (ADR 0029).
   - Rejected: mint and offer. Two devices that both create `work` before syncing get two lists.
   - Rename changes only the shown name (`name =` in the list's `txtodo.toml`, which syncs). The
     id stays.
   - `default` keeps its reserved constant. No migration.
3. **Lists sync only between your own devices.** Reuses pairing-consent option A
   (`ref:default-workspace-pairing-consent`, decided, not built).
4. **Repo workspaces leave txtodo sync.** Never offered, no relay or LAN route. The daemon still
   owns the file and takes in outside edits; a `git pull` is already one (`external.rs`
   `on_external_change`). Auto-mirror and `workspace bind` retire for repos.
5. **Inside a repo, the repo wins.** MCP and CLI register a git root on first use. The
   `mcp-cwd-autoregister` worry was the name being offered to peers, which 4 removes. A write that
   falls back to `default` says so in the tool result, not on stderr.
6. **A git merge driver.** `txtodo merge-driver %O %A %B`: 3-way per parsed line. Added on either
   side: kept. Done on either side: done. Edited on both: both kept, shown as an ADR 0032 duplicate
   group. `txtodo git setup` adds `todo.txt merge=txtodo` and `tasks/**/todo.txt merge=txtodo` to
   `.gitattributes`, `merge.txtodo.driver` to `.git/config`, and `.txtodo/` to `.git/info/exclude`.
   `doctor` suggests it.
   - Rejected: git's `merge=union`. Completion moves a line to the bottom, so union keeps both.
   - Rejected: auto-commit. It writes your git history without asking.
7. **Old repo mirrors retire, never deleted.** `doctor` lists them; `workspace prune
   --repo-mirrors` moves them aside with rejoin's `move_aside`.
8. **Finding repos: scan, first touch, explicit add.**
   - A repo list is a folder with a `.git` dir whose root holds `todo.txt` or `txtodo.toml`. A
     repo with no list is not registered until its first `txtodo add`. A linked worktree (`.git`
     is a file) is found only on first touch, never by the scan.
   - Scan roots default to `[$HOME]`, set with `txtodo repos roots add|rm`, stored in a
     `scan_roots` table in `registry.db` (no second config parser).
   - The walk copies `walker.rs`: explicit stack, `read_dir`, max depth 8. It skips
     `SKIPPED_DIR_NAMES`, hidden dirs, `~/Library`, `node_modules`, `target`, `vendor`, `dist`,
     `.cache` and the data dir.
   - Nested repos: on a find, record it and keep going down (skip only `.git`). Each nested repo is
     its own workspace; submodules too. The nearest `.git` still sets the root
     (`workspace_root_from`, unchanged).
   - Runs once in the background after the socket binds (`daemon-early-bind`), then every 6 h, and
     on `txtodo repos scan`.
   - Rejected: a `notify` watch on `$HOME`. Too heavy on FSEvents and inotify limits.
   - Ad hoc repos: first touch registers at once; a fresh clone is found by the next scan or first
     use; `txtodo repos add <dir>` covers a folder outside the roots.
   - A row whose root is gone is marked missing, never deleted; `workspace prune` already drops
     missing rows.
   - `txtodo repos` lists repo workspaces with the last scan time; `doctor` shows roots and count.

ADR-sized: ADR 0043 "Two kinds of workspace". Amends ADR 0029 (the default is one list of many).
Reverses the 2026-09-24 remote-mirror decision and the 2026-10-17 bind decision, for repos only.

## Slicing

Each sub-line is one commit at or under the gate's 300 diff lines, in file order.

## Verification

- Each slice: `just test`, clippy, the gate.
- e2e: two daemons both create `work` and converge to one list, no duplicates. A repo workspace on
  daemon A never shows on daemon B. Two clones complete and add lines, `git pull`, every line kept.
- `scripts/check-two-kinds.sh` (self-asserting, for a human): `txtodo lists new scratch`, add a
  line, assert it is in `<data dir>/lists/scratch/todo.txt`; then in a repo add via MCP and assert
  `git diff todo.txt` shows it.
- By hand: clone a repo holding a `todo.txt` under `~/Development`, `txtodo repos scan`, see it
  listed; `mkdir -p ~/x/y && cd ~/x/y && git init && txtodo add hi`, see it listed at once. On two
  Macs, add to `work` on one and see it on the other; edit a repo list, commit, push, pull on the
  other, see it in the checkout.

## Known state on the Mac that wrote this

- The txtodo checkout's `.txtodo/oplog.db` is in Tagged mode (old daemon). It is not registered;
  move `.txtodo/` aside before registering it again, or every list gets `id:` tags back. Those
  stamps are in `git stash` (`stash@{0}`), pure id tags, safe to drop.
- `remote/01M3ZK…` (txtodo) and `remote/01M4MJ…` (givens) are repo mirrors that slice 9 retires.
