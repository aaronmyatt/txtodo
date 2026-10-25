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

Decided by the agent 2026-10-25 on the human's brief; change any line. Reviewed and tightened the
same day (see "Review 2026-10-25" at the end for what moved and why).

1. **Two kinds, set by where the folder is. No new setting.**
   - List: in the data dir (`<data dir>/lists/<name>/`, plus `default/`). txtodo syncs it.
   - Repo: every other folder, usually a git checkout, and also every old `remote/<id>/` mirror.
     txtodo never syncs it; git does. A folder that is not a git repo is a Repo too, so it stops
     syncing at the cut; `txtodo lists import` is its way back (item 9).
   - Rejected: a per-workspace `sync = git|txtodo` flag. One more knob, and "both" is the bug.
2. **A list's id comes from its name.** `blake3("txtodo-list:" + name)`, cut to a `WorkspaceId`
   with a fixed past timestamp field (as the default's constant has), so it never equals a minted
   ULID, the default's reserved id, an alias, or the all-zero link sentinel.
   - Names are ASCII `[a-z0-9_-]`, 1 to 48 chars, lowercased on input, never `default`, never a
     Windows device name (`con`, `nul`, …). No Unicode, so no two devices disagree on
     normalisation, and the folder name is safe on every OS.
   - The id is permanent, like the default's: a golden test pins three names to three ids.
   - Two devices that both create `work` merge with no offer, as the default does (ADR 0029).
   - Rejected: mint and offer. Two devices that both create `work` before syncing get two lists.
   - Rename changes only the shown name (`name =` in the list's `txtodo.toml`, which syncs). The id
     and the folder name stay. `--list <x>` finds a list by shown name first, then by creation
     name.
   - `lists new work` refuses when id(`work`) exists under another shown name (it was renamed):
     it names the list it would have joined. Otherwise a new `work` silently merges into
     "projects".
   - `lists rm` is per device, like `workspace remove`: it unregisters and moves the folder aside,
     never deletes. The other devices keep the list. The mirror pass already never re-adds an id
     this device once held, so it does not come back.
   - `default` keeps its reserved constant. No migration.
3. **Lists sync only between your own devices.** The gate is built: pairing-consent option A shipped
   on 2026-09-24 (`ref:default-workspace-pairing-consent`, `devices.own_device`,
   `lan_session_gate.rs`, `workspace_catalog_mirror.rs`, the 2026-10-02 vouch carry-across). The work
   here is to run every list id through that gate, not only the default.
   - **A foreign peer gets no list at all.** Today it gets the default under an alias, as a Remote
     mirror. That path (`default_alias`, the alias swap, the skip pass) retires at the cut: a list is
     private, and sharing with another person is what git is for. This reverses a corner of the
     2026-09-24 human decision, for lists only; say so in the ADR.
   - Known gap, carried over: the file carrier is not gated (it has no peer to ask). The group key
     still gates it.
4. **Repo workspaces leave txtodo sync.** Never offered, no relay, LAN or file-carrier route. The
   daemon still owns the file and takes in outside edits; a `git pull` is already one
   (`external.rs` `on_external_change`). Auto-mirror and `workspace bind` retire for repos.
   `walker.rs` already leaves nested git checkouts to their own workspace (task
   `walker-nested-checkouts`), which is the rule item 8 relies on.
5. **Inside a repo, the repo wins.** MCP and CLI register a git root on first use. The
   `mcp-cwd-autoregister` worry was the name being offered to peers, which 4 removes, so this lands
   after the cut, not before. A write that falls back to `default` says so in the tool result, not
   on stderr; that half is safe now and ships first.
6. **A git merge driver.** `txtodo merge-driver %O %A %B`: 3-way per line, in `txtodo-core` as a
   pure `merge3`. The rules:
   - Lines are compared by exact text. Added on either side: kept. Deleted on one side and
     untouched on the other: deleted.
   - Done on either side: done. Match a done line to its open original by key: the text with `x`,
     the completion date, the `(A)` priority and the `pri:` tag removed.
   - Edited on both: both versions are kept as two lines, ours first. Nothing marks them. ADR 0032
     only flags byte-identical lines, so it does not show this. The point is that no content is
     lost; the user prunes. A later slice may mark them.
   - Edited on one side, deleted on the other: the edit is kept.
   - Order: base order, then each side's additions after their nearest surviving neighbour,
     ties broken by text. The result must not depend on which side is "ours", so two clones that
     pull each other end up with the same file. The proptest checks that, and "no line lost".
   - `done.txt` gets `merge=union` (it only ever grows). `notes.md` keeps git's text merge: a
     conflict there is the user's, as with any file.
   - `txtodo git setup` writes `todo.txt` and `tasks/**/todo.txt` `merge=txtodo` and `done.txt`
     `merge=union` to `.gitattributes` (committed), `merge.txtodo.driver` to `.git/config` (per
     clone) and `.txtodo/` to `.git/info/exclude`. The driver command uses the path as found on
     `PATH`, not the resolved one: a Homebrew symlink resolves into a versioned Cellar path that
     an upgrade removes. A clone that never ran setup gets git's own merge, which can conflict but
     cannot lose a line. `doctor` suggests setup.
   - Rejected: git's `merge=union` for todo.txt. Completion moves a line to the bottom, so union
     keeps both.
   - Rejected: auto-commit. It writes your git history without asking.
   - Later, not now: replaying both sides through the engine's sidecar reconcile for real
     field-level merges. It needs `txtodo-engine` in the CLI's dependencies and a symmetric
     tie-break for the stamps; the plain merge covers "never lose a line" first.
7. **Old repo mirrors retire, never deleted.** `doctor` lists each `remote/<id>/` mirror with the
   lines the same-named clone lacks, because a mirror can hold an edit that only ever reached this
   Mac that way. `workspace prune --repo-mirrors` moves them aside with rejoin's `move_aside`.
   `workspace bind` prints a pointer for one release, then goes.
8. **Finding repos: scan, first touch, explicit add.**
   - A repo list is a folder with a `.git` dir whose root holds `todo.txt` or `txtodo.toml`. A
     repo with no list is not registered until its first `txtodo add`. A linked worktree (`.git`
     is a file) is found only on first touch, never by the scan.
   - Scan roots default to `[$HOME]`, set with `txtodo repos roots add|rm`, stored in a
     `scan_roots` table in `registry.db` (no second config parser).
   - The walk copies `walker.rs`: explicit stack, `read_dir`, max depth 8. It skips
     `SKIPPED_DIR_NAMES` (today only `.git` and `node_modules`), hidden dirs, `~/Library`,
     `target`, `vendor`, `dist`, `.cache` and the data dir. Those extra names are the scan's own
     list, not an edit to the shared constant.
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
   - **Registering a repo writes `.txtodo/` into that repo's local exclude first** (the git dir's
     `info/exclude`, found with `git rev-parse --git-path info/exclude` so a linked worktree works).
     Otherwise the daemon drops an untracked `.txtodo/oplog.db` into a repo the user never touched,
     one `git add -A` from being committed. This is local to the clone and changes nothing tracked.
   - **Cost.** The loader opens every registered workspace, each with its own store and FSEvents
     watcher. A scan under `$HOME` can register dozens. Measure first (see Verification); if open-all
     is too heavy, open repos on first touch only. Not decided; the number decides.
9. **Moving an old synced folder to a list.** At the cut, a non-git folder that used to sync (and a
   `remote/<id>/` mirror whose origin was not a repo) stops syncing. `txtodo lists import <dir>
   [name]` copies `todo.txt`, `done.txt`, `notes.md` and `tasks/` into a new list. It never copies
   `.txtodo/`, so the list starts with a fresh log and no history. It ships with the lists, before
   the cut.
   - Rejected: re-keying the old workspace to a list id in place. Its ops carry the old id; a rekey
     is a wire and store migration for a corner case.
10. **MCP uses lists, never makes them.** A `list` param addresses an existing list by name. A call
    naming a missing one is refused with `txtodo lists new <name>` in the message. `lists
    new|rename|rm|import` join the allow-list test of commands MCP deliberately never mirrors
    (registry mutations), the same rule as `workspace add|remove`.
11. **Tests move first.** About a dozen two-daemon e2e files sync a plain folder workspace
    (`lan_live_push`, `relay_converge`, `nested_ref_sync`, `file_carrier_converge`, `layout_sync`,
    `relay_multiplex`, …). The cut would fail them all. The harness gets a helper that seeds its
    synced workspace as a list; the files move over while lists already sync, so they stay green
    the whole way. Rejected: a hidden "sync folders" daemon flag. It keeps the code the ADR retires
    alive for tests only.
12. **Ordering rule: nothing that stops a sync lands before its replacement.** Releases are cut
    from main and the lab runs every main commit, so any order that lands the cut early leaves the
    user's own repo lists unsynced on a release. Order in the sub-list: lists (additive), the git
    side, repo finding, then the cut. The lab uses the default workspace (`scripts/lab/lib/
    device.sh`), so it is not affected.

ADR-sized: ADR 0043 "Two kinds of workspace". Amends ADR 0029 (the default is one list of many).
Reverses the 2026-09-24 remote-mirror decision and the 2026-10-17 bind decision, for repos only, and
the foreign-peer alias corner of the 2026-09-24 pairing-consent decision, for lists.

## Slicing

Each sub-line is one commit at or under the gate's 300 diff lines, in one crate (the slice fence;
`apps/desktop` is unfenced). Phases are contexts, in file order:

- `@lists`: named lists and their clients. Additive; nothing is removed.
- `@git`: `merge3`, the merge driver, `git setup`. The replacement for repo sync.
- `@scan`: finding repos. Additive.
- `@cut`: the only phase that removes behaviour. Starts with moving the tests, ends with retiring
  the mirrors and bind.

The CLI and TUI parity rows (`specs/client-parity.toml`, ADR 0031) ride in the same commit as the
keys they describe. A client that lags gets an `@parity` line.

## Verification

- Each slice: `just test`, clippy, the gate.
- e2e: two daemons both create `work` and converge to one list, no duplicates. A list made on A
  reaches B with no accept. A foreign peer gets no list. A repo workspace on daemon A never shows on
  daemon B. Two clones complete and add lines, `git pull`, every line kept.
- Lab: `partition-lists` (both devices make `work` while apart, edit, reconnect, converge).
- `scripts/check-two-kinds.sh` (self-asserting, for a human): `txtodo lists new scratch`, add a
  line, assert it is in `<data dir>/lists/scratch/todo.txt`; then in a repo add via MCP and assert
  `git diff todo.txt` shows it.
- Cost check before the scan ships: note daemon RSS and boot-to-ready time with this Mac's 10 repos
  under `~/Development` registered, against now.
- By hand: clone a repo holding a `todo.txt` under `~/Development`, `txtodo repos scan`, see it
  listed; `mkdir -p ~/x/y && cd ~/x/y && git init && txtodo add hi`, see it listed at once. On two
  Macs, add to `work` on one and see it on the other; edit a repo list, commit, push, pull on the
  other, see it in the checkout.
- Upgrading the two real Macs, in this order: commit and push every repo's `todo.txt` on both; upgrade
  both; `txtodo git setup` in each clone; `git pull` so the clones converge; read `doctor`'s mirror
  rows for lines only a mirror holds; then `workspace prune --repo-mirrors`.

## Check before you start (per Mac)

The first version of this note recorded one Mac's state. Re-check on the Mac you are on:

- `git stash list` and `git stash show -p stash@{0}`. On the Mac that wrote the review, `stash@{0}`
  was the daemon KeepAlive fix WIP (`service.rs`, `service_tests.rs`, the plist), not id stamps.
  Do not drop a stash without reading it.
- Whether the checkout is registered: `sqlite3 -readonly ~/.local/share/txtodo/registry.db 'select
  * from workspaces'` (read-only; a daemon-mode `txtodo` call from a worktree registers that
  worktree). On the Mac that wrote the review, `/Users/oya/Development/txtodo` was registered.
- If its `.txtodo/oplog.db` is in Tagged mode, move `.txtodo/` aside before registering, or every
  list gets `id:` tags back.
- `remote/` folders there are repo mirrors that the cut retires. On the Mac that wrote the review
  they were `01M4MJ…`, `01M4R2…` and `01M5P2…`; the `01M3ZK…` named under Why was not there.

## Review 2026-10-25 (agent)

What changed in the plan, and what I chose between.

- **Order.** Before: the cut (old line 3) came third, before the git side (old line 9) and the
  finder (old line 8). Now it comes last. Chosen over keeping file order because it avoids a window
  where repo lists sync by nothing.
- **Size.** Old lines 3, 5, 8, 9 and 10 each spanned two to four crates and could not be one commit
  under 300 lines. Now one crate per line, 35 lines in four phases.
- **Wrong claims fixed.** Pairing-consent was called "not built"; it is built. Edited-on-both lines
  were said to show as ADR 0032 groups; that ADR flags only byte-identical lines. The stash and the
  registry state were one Mac's, now a checklist.
- **Gaps filled.** How a list made on one device reaches the others (an own device's offer creates
  `lists/<name>/`). Name rules and permanence. Rename collision and `lists rm` meaning. What MCP may
  do with lists. Migration for a folder that stops syncing (`lists import`). The test harness. The
  merge rules and their symmetry. `.txtodo/` exclude on registration. Scan cost. The driver path
  under Homebrew upgrades. `ref:bind-ui` (open) and the web-pwa note ("a browser workspace is a
  Remote mirror") both conflict with this plan.
- **Options picked.**
  - Foreign peers: no list (picked) vs the alias mirror the default has today.
  - MCP: use lists only (picked) vs let agents create them.
  - Folder that stops syncing: `lists import` copy (picked) vs re-key in place.
  - Merge: plain text-keyed three-way (picked) vs engine replay (later).
  - Tests: move the harness to lists (picked) vs a hidden daemon flag.

## As built

- **mcp fallback note.** A call that names no `workspace` on a server that fell back to the default
  now ends with a second text block saying so (`fallback.rs`, `McpServer::with_fallback_note`;
  `call_tool` is overridden because `#[tool_handler]` skips a method that is already defined). It
  covers the whole tool set in one place. Not done: the `--global` and `--dir` targets add no note,
  since they never fall back. The text still names `txtodo workspace add`; the git-root
  auto-register line changes it.
