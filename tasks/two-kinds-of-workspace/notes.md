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
     its own workspace. The nearest `.git` still sets the root (`workspace_root_from`, unchanged).
     (A submodule has a `.git` *file*, so like a worktree it is a first-touch find, not a scan find;
     the sentence here once said otherwise.)
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
- **paths kind.** `txtodo-workspace-paths::{WorkspaceKind, lists_dir_for, workspace_kind}`
  (`kind.rs`). The kind is read off the root, never stored: the default folder or anything inside
  `lists/` is a List, every other folder is a Repo. Roots are compared as given and canonical
  (the registry stores canonical roots; `/var` is `/private/var` on macOS), and the lists dir need
  not exist yet. `lists_dir_for` follows an isolated daemon's `$TXTODO_SOCKET` like
  `default_workspace_dir_for`. A moved default (`$TXTODO_DEFAULT_WORKSPACE`) is still the List.
- **proto.** `WorkspaceInfo.kind` (field 13, `WorkspaceKind` enum, UNSPECIFIED from an older
  daemon) and `ListEnsure`, `ListRename`, `ListRemove`, device-level with no selector. Choices:
  - `ListEnsure` returns `{ workspace, created }` so `lists new` can say "exists" without a second
    call; it is create-if-absent, so `--list <name>` stays a lookup and a typo cannot make a list
    that then syncs to every device.
  - `ListRename`/`ListRemove` take the list's shown name, else its creation name, and the daemon
    resolves it. Ordinary commands (`--list work`) resolve client-side from `WorkspaceList`
    (`kind` + `name` + the root's folder name) rather than a new `WorkspaceSelector` member: a
    daemon that does not know a new oneof member reads the selector as absent and would write to
    the sole open workspace, which is the wrong-list bug this task exists to stop.
  - `ListRemove` has no `execute`/`dry_run`: it moves the folder aside and never deletes.
  - Known gap: the daemon does not build between this commit and the daemon's list handlers (both
    `Txtodo` impls need the new methods, same split as `WorkspaceRename`); the next daemon commit
    starts with the stubs.
- **daemon lists (lines 5 to 7, one commit).** `list_workspace.rs` (`ListName`, `id()`),
  `workspace_catalog_lists.rs` (`ensure_list`, `find_list`, `remove_list`), `list_rpc.rs` (the three
  handlers), `boot_dirs.rs` (sets `lists/` beside `remote/`), `kind` on `WorkspaceInfo`, and the
  no-rekey guard widened to list roots. Landed as one commit because each piece is dead code
  until the next uses it, and clippy runs with `-D warnings`.
  - Golden ids: `work` 01K88NT001XV65VHTNCYP6PE6F, `home` 01K88NT0015SHKXH8JFCPTKBMD,
    `a-b_c9` 01K88NT001NFC98TAGX7K9P9Q9 (all start `01K88NT001`, the fixed timestamp field).
  - A list removed here and made again needed `Registry::restore` in `txtodo-store` (separate
    commit): the registry tombstones a removed row, and `adopt` treats that row as "already
    adopted" and leaves it removed. The same quirk would have kept a rolled-back pairing rekey
    removed; left alone, noted here.
  - `lists new X` is refused (`ALREADY_EXISTS`) when another list already shows `X`, not only when
    `X`'s own id was renamed away, so two lists never show one name through this path. Two devices
    renaming at once can still produce two lists with one shown name; `find_list` then refuses as
    ambiguous and the creation name reaches either.
  - To keep the file-length gate, `GlobalService::scoped/resolve` moved to
    `global_service_helpers.rs` and `TxtodoService::new/workspace` to `server_actors.rs`.
  - Known gap: lists reach other devices only through the existing offer path, which still creates
    `remote/<id>/` mirrors until the "an own device's list offer makes lists/<name>/" line lands. A
    list made on A shows on B as a Remote mirror in the meantime, and that mirror's shown name is
    the offered one.
- **list sync gate.** `lan_session_gate::session_routes` drops every list route for a peer that is
  not own, with no alias in its place (a list is private; sharing is git). A list is told from any
  other workspace by id alone, `list_workspace::is_list_id` (the fixed timestamp field), because
  the gate has no catalog to ask. Known gaps:
  - The control channel still offers list names to every peer in the group: the sender does not
    know who it talks to on an accepted session, and reordering the exchange risks both sides
    waiting on the other. The receiving side ignores a foreign list offer (next line), so only the
    name leaks, never content.
  - The file carrier (`--sync-dir`) has no peer to ask and still carries lists to anyone reading
    the folder, as it does the default (ADR 0029's known gap). The group key still gates it.
- **list offers.** `WorkspaceCatalog::mirror_offered_list` (`workspace_catalog_lists.rs`): an offer
  whose id is a list id (`is_list_id`) from an own device makes a folder in the lists root,
  registers it under the offered id and opens it; from any other device it is ignored (debug log,
  consumed). The offer carries the shown name, not the name the list was made under, and the
  receiver cannot recover that from the id's hash. So the folder is named after the shown name when
  that is a free list name, else after the id in lowercase; the id carries the identity either way.
  Effects: on a device that received a renamed list, the "made under" name that `find_list` falls
  back to is the shown name at the time, and `lists new <old name>` still refuses by id. Manual
  `accept_offer` of a list goes through the same path and refuses a foreign one
  (`FAILED_PRECONDITION`). The existing skip rule still holds: a list removed here is not made
  again by the next offer (only `lists new` brings it back).
- **lists e2e.** `tests/e2e/lists_sync.rs`, two real global-mode daemons paired over LAN
  (`--profile ci`, about 6 s each). Proven end to end: both make `work` before they meet and end
  with one list holding both devices' lines once each; a list made on A after pairing reaches B
  under `lists/` as a List, not a Remote mirror, with no accept; with B marked not own, B gets A's
  default as an alias mirror (so sessions and offers ran) and no list, none of its text. Not run
  here: a relay-only pair, or three devices.
- **cli lists.** `txtodo lists [ls]|new|rename|rm|import` and `--list NAME` (`commands/lists.rs`,
  `lists_import.rs`; client calls in `client_workspace.rs`). Choices:
  - The CLI already had a hidden global `--list FILE` (the sub-list `sub` passes). It is now
    `--sublist`; `--list` is the named list. `sub`, the register guard and the docs were updated.
    Nothing outside this crate used the old spelling.
  - `--list NAME` is `--dir <that list's folder>`: the CLI asks the global daemon (starting it if
    down) via `WorkspaceList` and applies the shared `txtodo_workspace_paths::pick_list` rule, then
    carries on exactly as `--dir` would. It conflicts with `--dir`, and refuses `--no-daemon`
    (a list's folder is only known to the daemon). Cost: one extra local RPC per `--list` call.
  - `pick_list` is one function in the paths crate, used by the daemon's `find_list` and the CLI
    (MCP next), so the rule cannot drift.
  - `lists rm` has no confirm prompt: it moves the folder aside and prints where, never deletes,
    and the other devices keep the list.
  - `lists import DIR [NAME]` copies files, skips hidden entries and symlinks, and refuses an
    existing list or a folder with no `todo.txt`. If the folder had a `txtodo.toml` the list is
    renamed to its own name afterwards, so an old `name =` line does not show.
  - Known gaps: `import` is a plain copy, so a failure part way leaves a half-filled new list (the
    message says so); import does not read a custom `todo_file` layout, only `todo.txt`; no shell
    completion for list names.
- **mcp list argument.** Every tool that takes `workspace` also takes `list` (`list_param.rs`).
  - Not 15 new struct fields: `call_tool` rewrites a `list` argument to the list's `workspace` id
    before any tool runs (asking the daemon for the workspaces, then `pick_list`), and
    `McpServer::new` adds the `list` property to each tool's published schema so agents see it.
    `fallback.rs`'s note logic moved into the same path (`list_param::call`), after the rewrite, so a
    call that named a list gets no fallback note.
  - A name that is no list (a repo included), or a list beside a `workspace`, is `invalid_params`.
    The refusal names the lists there are, so an agent can retry; there is no tool to list them,
    and that is on purpose (`todo_list_workspaces` is not in the allow-list).
  - `lists new|rename|rm|import` stay out of the tool set: `NEVER_MIRRORED` in `smoke.rs` gained
    `lists`, and `CLAUDE.md` says why.
  - `backend::WorkspaceInfo` gained `is_list` (from `WorkspaceInfo.kind`); false from an older
    daemon, so an older daemon has no lists to name.
- **tui lists.** The `W` popup and Settings › Workspaces show each workspace's kind (`list` or
  `repo`, nothing from an older daemon) and put lists first. Choices:
  - No new global keys. This TUI does workspace management as rows in Settings › Workspaces, so
    "keys for new, rename, remove" became: a `New list` field row (`ListEnsure`), `d d` on a list
    row (`ListRemove`: folder moved aside, other devices keep it) and the existing rename field
    (`ListRename` for a list). Both route by `WsKind::List`; a repo or a mirror keeps the old calls.
    A list is addressed by the folder name it was made under, not its shown name, since a shown
    name need not be unique.
  - Sorting happens in `WorkspaceMenu::fill` and in the Settings rows, not in `menu_items`, because
    `refresh` zips the daemon's list with those items by position.
  - Manifest: new row `workspace.new_list` (TUI done; desktop planned, web na). The desktop line in
    this task is its `@parity` backlog line.
  - Known gaps: renaming still reaches only the open workspace (existing deviation); a list's
    rename field starts from the shown name and an empty value is refused by the daemon (a list
    needs a name); no repo-vs-list grouping header, only the tag and the order.
- **desktop lists.** The header workspace menu groups its rows under Lists and Repos
  (`workspaceGroups.ts`, pure and unit-tested; no kinds from an older daemon means no grouping, as
  before), has a `New list` field, and routes a list's rename and remove through `ListRename` and
  `ListRemove`; Settings › Workspaces routes a list's remove the same way, with its own confirm text.
  Rust side: `WorkspaceInfoDto.kind`, `new_list`/`rename_list`/`remove_list` Tauri commands, the
  `e2e_bridge` arms and the dev mock's answers (a vitest checks the mock answers every command).
  - A list is addressed by its folder name (`folderOf(root)`), as in the TUI.
  - The menu's new-list input is labelled `New list name`, not `Name for …`: the existing rename test
    finds its input by that prefix.
  - Settings › Workspaces gets no kind badge. The visual goldens (`settings-workspaces-*`) would
    change, and re-baselining goldens is a human's call (`playwright.config.ts`); say so if you want
    the badge. The open menu is in no golden.
  - Manifest `workspace.new_list`: desktop done, TUI done.
  - Not checked by eye: this session never opened the app. The functional Playwright specs for the
    menu cover it with DOM assertions; how the Lists/Repos headings look is for a human to see.
- **core merge3.** `txtodo_core::merge3(base, ours, theirs)` (`merge3.rs`), pure, `no_std` + `alloc`.
  Per distinct line the result holds `base - max(deleted by either side) + max(added by either
  side)` copies. That is the whole rule set: additions on either side stand, a completion (the open
  line leaves, a new `x …` line arrives) leaves the done line only, two different edits of one line
  both stay, edit beats delete, and the same change on both sides is one line.
  - **Correction to the Design section.** It said the result must not depend on which side is
    ours, so two clones end with the same file. Order cannot be both that and "follow ours": a side
    that archived (reordered) its file would have the reorder undone by a base-order merge. The
    result now follows `ours`; only the set of lines is symmetric (a proptest checks it). Two clones
    that merge each other independently can differ in order, not in content.
  - Known limits, by design: no identity, so two edits of one line are two lines; a line completed
    on one side and edited on the other leaves the done line and the edited open one (nothing is
    lost, the user prunes); the same task completed on two days gives two done lines.
  - Proptests: the lines do not depend on which side is ours; no added line is lost and none is
    invented; an unchanged side hands the other file back exactly.
- **lab partition-lists.** `scripts/lab/scenarios/partition-lists.sh` sets `WS` to the list's folder
  after the first step, so the lab's own convergence, no-loss and doctor checks look at the list.
  Run on 2026-10-25 against the working tree (`LAB_NO_TASK=1 scripts/lab/lab.sh start
  partition-lists`, run 20261025-184118, seed 1040902651, image `txtodo-lab:src-b9df59ae931a`): pass
  in 13 s. Both containers ended with all 30 added lines in `work`, each once, after both made
  `work` while b1 was unplugged; `scratch` made on a1 reached b1 unasked; each device lists exactly
  one `work`. One attempt, one seed: not a flake count.
- **cli merge-driver.** `txtodo merge-driver BASE OURS THEIRS` (`commands/merge_driver.rs`) reads the
  three files, runs `merge3`, writes the result over OURS. A missing ancestor is an empty one; a
  file that is not UTF-8 is refused with exit 1, which git reads as a conflict to settle by hand, so
  a binary-looking file is never half-merged. Handled in `run()` before paths, config and daemon.
  `tests/it/merge_driver_git.rs` is the "real git merge in a temp repo": both branches edit one
  line and add at the end, git's own merge conflicts (a control test shows it), and through the
  driver the merge finishes with every line, none of the old one, no markers. It skips when `git`
  is not installed.
- **cli git setup.** `txtodo git setup [--dry-run]` (`commands/git_setup.rs`). The clone is the
  nearest folder at or above `--dir`/cwd holding `.git`. `plan` is pure (so the doctor row reuses
  it); `apply` appends to `.gitattributes` under a header comment, appends `.txtodo/` to the
  exclude file that `git rev-parse --git-path info/exclude` names (so a linked worktree writes the
  main clone's), and sets the driver with `git config --local`.
  - The driver path is `txtodo` as found on `PATH` when that file is this binary, else the running
    binary (a dev build). Quoted for the shell only when it needs it.
  - `.gitattributes` also names the root list when `txtodo.toml` gives another (`work.txt`).
  - A new `commands/early.rs` answers `upgrade`, `merge-driver` and `git` before paths, config and
    daemon; it also freed lines in `main.rs`, which sat at its 400-line limit.
  - Tests: pure plan and quoting; a real temp clone: three things written, second run a no-op, dry
    run writes nothing and keeps existing attributes, a folder with no clone is refused, a linked
    worktree; and `tests/it/merge_driver_git.rs` runs `txtodo git setup` then a real `git merge`.
  - Known gap: the other clones still have to run it (the driver config is per clone by git's
    design); the doctor line next makes that visible.
- **cli doctor git row.** `doctor_git.rs`: one WARN `git` row per registered repo workspace that is
  a clone (`.git` beside it) and has something `txtodo git setup` would still write; it lists what
  is missing and names `txtodo git setup --dir <root>` (then commit `.gitattributes`). No row for a
  clone that is set up, a list, a mirror, a plain folder, or when `git` is not on `PATH`. WARN, not
  FAIL: a clone without the driver still works, git's own merge can conflict and cannot lose a line.
  `plan_for` was split out of `run` so both ask the same question.
- **cli two-clones test.** `tests/it/git_two_clones.rs`: a bare remote, clones `a` and `b`, each with
  `txtodo git setup`, working their lists with the real CLI in direct-file mode (`--no-daemon`, so
  no daemon). A completes `alpha` and adds two; B completes `bravo` and adds one; B `git pull`s
  (a true merge, the driver settles it), pushes, A pulls. Every added line is there, `alpha` and
  `bravo` each exist once, as done, none open, no conflict markers, and the two files are equal at
  the end. It stands in the `it` target (no daemon, about a second) rather than `e2e`.
  Not covered: the daemon watching the working tree through the pull (the existing external-edit
  path takes in a rewritten file; a lab or by-hand check on a registered repo would show it).
- **store scan state.** `Registry::{scan_roots, add_scan_root, remove_scan_root, seed_scan_roots,
  last_scan_ms, set_last_scan_ms}` (`registry_scan.rs`). Two tables in `registry.db`: `scan_roots`
  and a small `scan_state` (the seeded flag and the last scan time).
  - **Not a numbered migration.** The tables are `CREATE TABLE IF NOT EXISTS` at every open, so the
    registry's `user_version` stays 2 and an older build (the app's bundled daemon, say) keeps opening
    the file; a bump to 3 would make it refuse the whole registry. A test pins `user_version` at 2.
    This departs from the notes' "scan_roots table in registry.db" only in how it is made.
  - Seeding is once-only (`roots_seeded` in `scan_state`): a fresh registry gets `[$HOME]`, and a
    person who removes every root is not given it back. Empty means scan nothing.
  - Roots must be absolute (a leading `/` or a drive letter), at most 4 KiB, at most 64 of them.
    Two new `StoreError`s: `TooManyScanRoots`, `BadScanRoot`.
- **proto repos.** Two RPCs, `Repos` and `MirrorsRetire`, not the four the todo line named.
  - `Repos { add_roots, remove_roots, scan, wait }` answers `{ roots, last_scan_ms, running, found,
    registered }`: one RPC for the whole scan family (list, add, remove, scan now), and all-empty
    reads. Fewer methods also matters here: `server.rs` and `global_service.rs` sit at their
    400-line limit and every RPC costs each of them a 6-line method.
  - `WorkspacePrune.repo_mirrors` named a RPC that does not exist (`workspace prune` is CLI-side over
    `WorkspaceList` and `WorkspaceRemove`), and a mirror must be moved aside by the daemon (close,
    move, un-register). So it is a new `MirrorsRetire { workspace_id, dry_run }` returning the
    `RetiredMirror`s. The `workspace prune --repo-mirrors` CLI flag, when it comes, calls it. The
    later todo lines were reworded to match.
  - As with the list RPCs, the daemon does not build between this commit and its handlers.
- **daemon repo scan.** `repo_scan.rs` (pure walk), `workspace_catalog_repos.rs` (register, schedule,
  report), `repos_rpc.rs` (the `Repos` RPC), `repo_exclude.rs`, and the `Repos` handler plus a
  `MirrorsRetire` stub that keep the daemon building after the proto commit. Landed as one commit:
  each piece is dead code without the next.
  - **A scan only registers.** Nothing is opened, so thirty clones cost thirty registry rows and no
    watchers. The loader opens them at the next start after the workspaces in use, and a first use
    opens one at once. This answers the notes' open cost question for now: the number is not
    measured, because nothing is loaded; if open-on-start proves heavy, the loader can skip
    repos until used.
  - **Never walks a test's home.** `[$HOME]` is seeded as the default root only when neither
    `TXTODO_SOCKET` nor `TXTODO_REGISTRY_DB` is set. Every test and harness sets them, so an
    isolated daemon has no roots and its scheduled pass does nothing. Seeding is once-only
    (`seed_scan_roots`); a person who removes every root is not given `$HOME` back.
  - **A removal sticks.** `root_was_removed` (new in the store, its own commit) stops the scan
    re-registering a clone the person took off. A nested clone is its own workspace: the walker
    already leaves nested checkouts to themselves, so the registry reports no overlap (a test pins
    this; my first guess was wrong).
  - **`.txtodo/` goes in the clone's own `info/exclude`** from `WorkspaceRegistry::add`, for every
    new registration of a git folder (scan, `workspace add`, first touch), without running `git`
    (the daemon's `PATH` may lack it): `.git` folder, or `gitdir:` file then `commondir`.
    Lists and mirrors have no `.git`, so they are untouched.
  - Scan roots are canonicalised before the walk, so the daemon's own data folder (skipped by its
    parent of `lists/` and `remote/`) is recognised even when the root was written through a symlink.
  - To fit `server.rs` and `global_service.rs` under the 400-line cap, the new trait methods (and
    the earlier list ones in `server.rs`) are one `#[rustfmt::skip]` line each.
- **cli repos.** `txtodo repos [ls]|scan [--background]|add DIR|roots [add|rm DIR]`
  (`commands/repos.rs`) over the `Repos` RPC, and a `repos` OK row in `doctor` (count of repo
  workspaces, scan roots, last scan; `doctor_git.rs`). `scan` waits and prints `found N, registered
  M new`. `add DIR` refuses a folder with no `.git` (pointing at `workspace add`) and otherwise is
  `WorkspaceAdd`, which also writes the clone's `.txtodo/` exclude. `roots rm` accepts a folder that no
  longer exists (a root can outlive its folder). `tests/e2e/repos.rs`: a real daemon with no seeded
  root scans nothing until a root is added; then a clone is found, registered, listed, doctor warns
  `txtodo git setup --dir …`, `git setup` clears it, and the root comes off.
  - `Command::is_registry` replaced two copies of the `Workspace | Lists` match in `main.rs`, which
    sat at its 400-line cap.
- **daemon e2e harness.** Every `support::Daemon` now serves its `--dir` as a list, so the two-daemon
  tests keep syncing once a plain folder stops. The `--dir` bridge keeps its state dir inside the
  `--dir`, so its default `lists/` can never hold the `--dir`; `$TXTODO_LISTS_DIR` (read in
  `boot_dirs.rs`, bridge only) moves that folder. `seed.rs::list_dirs` makes a lists folder and a
  `work-*` folder inside it; `Daemon` keeps both alive. Global-mode tests use
  `multi::list_dir_in(registry_dir, prefix)`, which makes the folder in `<registry dir>/lists/` (where
  `lists_dir_for` already puts it). Seeded ids stay arbitrary, not name-derived: the gates that matter
  to these tests key on the folder (`is_list_root`), and `lists_sync.rs` covers the name-derived path.
  - The seeded constructors moved to `support/starts.rs` because `support/mod.rs` was at the cap.
  - Not moved, on purpose, because each tests the repo-mirror path the cut removes, and a list is
    not a mirror: `workspace_rejoin`, `workspace_name` (own line, rewrite on `ListEnsure`) and
    `workspace_bind` (goes with bind).
  - The cli's `pairing.rs` converge test cannot just swap its folders: it relies on the joiner's
    folder adopting the initiator's id (`adopt_offered_workspace_id`), and a list keeps its own id.
    It becomes "pair, then A's list arrives on B" (own line).
  - Whole daemon e2e suite on the list-backed harness: 66 passed before the multi-workspace edits;
    the four `file_carrier_converge`/`relay_multiplex` tests rerun green after.
- **rejoin and name tests on lists.** `workspace_rejoin.rs`: A makes `work` with `ListEnsure`, adds its
  two lines through `Apply`, and B finds the list it was offered (`list_with`, a List that is not the
  default); the rest (dry run, backup beside the root, refilled from A, B's next edit reaches A) is
  unchanged and passed first time, so rejoin needs nothing for lists. `workspace_name.rs`: the list
  `plants` shows on B under the same name in a folder called `plants` (not the id), and a rename on
  either side reaches the other through `txtodo.toml`. The mirror's offered name path
  (`offered_name`, the "owner's offer carries the name" race) has no test now; it is mirror-only and
  retires with them.
- **cli pairing test.** `a_paired_joiner_receives_the_initiators_real_file` went: it proved the
  joiner's folder adopts the initiator's workspace id, and a list never re-keys (nor does a plain
  folder sync after the cut). `tests/e2e/pairing_lists.rs` is the CLI-level replacement, through the
  global daemon: `lists new work`, `--list work add`, `txtodo pair` from the default workspace with
  "yes, yes" on both ends, then B's `lists/work/todo.txt` holds the line and `lists` shows `work`.
  `GlobalDaemon` gained `spawn_with` (env, for a 1 s redial) and `command` (the `txtodo` Command it
  already ran, for stdin). The ceremony tests that stay in `pairing.rs` (QR, SAS, refusals) still run
  on `--dir` daemons: they test the ceremony, not what syncs after it.
- **the cut: repos leave sync.** Three places, one test each.
  - **Offers** (`control_session.rs::outbound_offers`): only a list is offered, by id shape
    (`is_list_id`: the control session has no catalog to ask). That also stops offering the
    **default**, which I had not planned to touch here: it was only ever offered under an alias for a
    foreign peer to mirror, and an own device skips it. A mirror of a repo-kind folder gets no route
    now, so the alias mirror would have arrived empty. So the foreign default is gone with this line
    (the retire line is left with the receiving side and the alias helpers).
  - **Routes** (`workspace_catalog_open.rs`): `open_workspace_full` takes `syncs`, from
    `WorkspaceCatalog::syncs` (the default, or inside the lists root). Only then does it register on
    the relay, file-carrier and LAN tables. `relay::start` still runs for every workspace, because
    it sets the device's relay status (`Health.relay_url`, "bound as …"), and a device whose open
    workspaces are all repos should still say so; its dial task is only started for a syncing one,
    since the task would die with a repo.
  - **Pairing** (`adopt_offered_workspace_id`): the loop that moved a rekeyed workspace's routes is
    gone. Only a repo reaches the rekey now (the default and lists return first), and a repo has
    none. The rekey itself (a joiner's empty folder taking the initiator's id) is now meaningless and
    is its own deletion line.
  - Tests: `control_session_tests::only_lists_are_offered`; `lists_sync::a_repo_is_never_offered_and_
    never_reaches_the_other_device` (a plain folder added on A, a list beside it as the clock: once
    the list is on B, four more redials, and B holds no mirror and none of the repo's text);
    `a_device_that_is_not_own_gets_no_list` now expects nothing at all for a foreign device, with
    "a session started on both logs" as its clock. Removed with the behaviour they proved:
    `default_workspace_foreign` (foreign default as a Remote mirror), `default_workspace_pairing`
    (a joiner gaining an offered folder as a mirror), `workspace_bind` e2e. `two_devices.rs` is what
    is left of the first: the pairing helpers `lists_sync` uses.
  - Known gaps: nothing gates the file carrier for a foreign peer (carried over); the lists' names
    still ride offers to a foreign peer in the group (they have no route there).
