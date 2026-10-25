# 0043 — Two kinds of workspace: lists that txtodo syncs, repos that git syncs

- Status: accepted 2026-10-25 (decided by the agent on the owner's user stories; plan in
  `tasks/two-kinds-of-workspace/notes.md`). Amends ADR 0029. Reverses the repo half of the
  2026-09-24 remote-mirror decision and of the 2026-10-17 bind decision, and the foreign-peer alias
  corner of the 2026-09-24 pairing-consent decision, for lists only.
- Date: 2026-10-25
- Deciders: agent, on the owner's brief

## Context
Two things the owner wants, and one bug that showed they were mixed up. Checked on one Mac on
2026-10-25:

- Any number of named lists that follow the user to their other devices, like the default does.
- A repo's `todo.txt` and `tasks/` that travel with git, so a developer never loses content in
  their normal flow.
- An MCP edit made inside a repo went to the default list, not the checkout. The checkout had no
  registry row, and the only notice was on stderr, which agent hosts hide. The repo's real synced
  list sat in a Remote mirror under the data dir that the checkout never saw. A clone on a second
  device mints its own workspace id, so it never merges with the first device's list; `workspace
  bind` fixes that by hand, one repo at a time.

git already carries a repo's files. Syncing them a second way gives two truths.

## Decision
1. **Two kinds, set by where the folder is.** A **list** lives in the data dir:
   `<data dir>/lists/<name>/`, plus `default/`. txtodo syncs it. A **repo** is every other folder,
   usually a git checkout, and every old `remote/<id>/` mirror. txtodo never syncs it; git does.
   There is no `sync =` setting.
2. **A list's id comes from its name.** `blake3("txtodo-list:" + name)` cut to a `WorkspaceId`
   whose timestamp field is a fixed past constant, so it never equals a minted ULID, the default's
   reserved id, an alias, or the all-zero link sentinel. Names are ASCII `[a-z0-9_-]`, 1 to 48
   characters, never `default`. The derivation is permanent, like ADR 0029's constant. Two devices
   that both create `work` converge with no offer. Rename changes only the shown name (`name =` in
   the list's `txtodo.toml`); the id and the folder name stay.
3. **Lists go only to your own devices.** Every list id runs through the own-device gate ADR 0029's
   amendment built for the default. A device that is not yours gets no list at all. Today it gets
   the default under an alias as a Remote mirror; that path retires. Sharing with another person is
   what git is for.
4. **An own device's list offer creates `lists/<name>/`** on the receiver, not `remote/<id>/`. The
   receiver checks the name before it makes a folder.
5. **Repo workspaces leave txtodo sync.** Never offered; no relay, LAN or file-carrier route. The
   daemon still owns the file and takes in outside edits; a `git pull` is one. Auto-mirror and
   `workspace bind` retire for repos. Old repo mirrors are listed by `doctor` and moved aside by
   `workspace prune --repo-mirrors`, never deleted.
6. **Inside a repo, the repo wins.** The CLI and MCP register a git root on first use. Registering
   was unsafe while the name was offered to peers (task `mcp-cwd-autoregister`); item 5 removes
   that. A call that falls back to the default says so in the tool result.
7. **A git merge driver.** `txtodo merge-driver %O %A %B` merges `todo.txt` three ways per line and
   never loses a line; `txtodo git setup` installs it. `done.txt` takes git's `union`.
8. **Finding repos.** The daemon scans configurable roots (default `$HOME`) in the background for
   folders with a `.git` and a `todo.txt` or `txtodo.toml`, registers them as repos, and a first
   touch registers one at once.
9. **A folder that stops syncing has a way back.** `txtodo lists import <dir> [name]` copies its
   files, not its op log, into a new list.

## Consequences
- Good:
  - Lists and repos each have one carrier, so there is one truth.
  - A clone on a second device needs no bind; git carries the file.
  - A list created on two devices cannot split into two.
  - An agent in a repo writes to the repo, and a fallback is visible.
- Bad:
  - A plain folder that is not a git repo, and any list shared with another person, no longer
    syncs through txtodo. `lists import` is the move for the first; there is no move for the
    second.
  - A list's id is its name forever. Renaming keeps the old id, so a later `lists new` of the old
    name is refused, not merged.
  - Edits made on both sides of a repo list merge by keeping both lines. Nothing marks them.
  - Importing a folder loses its op history.
- Neutral / follow-ups:
  - The file carrier still has no peer to ask, so it stays ungated (ADR 0029's known gap).
  - The scan registers repos the loader then opens; the cost on a machine with many repos is
    measured before the scan ships.
  - Replaying both sides through the engine for field-level merges is a later option.
  - The web PWA (ADR 0042) holds lists, not mirrors.

## Alternatives considered
- **A per-workspace `sync = git|txtodo` flag.** One more knob, and "both" is the bug.
- **Mint a list's id and offer it.** Two devices that both create `work` before syncing get two
  lists.
- **Give a foreign peer the list under an alias, as the default does today.** Keeps a second
  sharing path that git already covers, and a list is private by intent.
- **Re-key an old synced folder to a list id in place.** Its ops carry the old id; a rekey is a
  wire and store migration for a corner case.
- **Git's `merge=union` for `todo.txt`.** Completion moves a line to the bottom, so union keeps
  both copies.
- **Auto-commit repo lists.** It writes the user's git history without asking.
- **A `notify` watch on `$HOME` to find repos.** Too heavy on FSEvents and inotify limits.
- **A hidden daemon flag so tests keep syncing plain folders.** It keeps the code this ADR retires
  alive for tests only. The test harness seeds a list instead.

## As built (2026-10-25)
What shipped, and where it differs from the text above. The per-line record is
`tasks/two-kinds-of-workspace/notes.md`.
- **Decision 3 went further than written.** The default is no longer offered either: it only ever
  went out under an alias for a foreign peer to mirror, and an own device skips it. A foreign peer's
  sync session now carries neither the default nor a list; `default_alias`, the foreign-default
  Remote mirror and its skip pass are deleted. Only a list is offered, by id shape.
- **Decision 5.** A workspace registers relay, LAN and file-carrier routes only if it is the default
  or inside the lists folder (`WorkspaceCatalog::syncs`). An offer that is not a list, from an older
  build, is consumed and ignored. Pairing no longer re-keys a joiner's folder to the initiator's id
  (that code is deleted). `MirrorsRetire` (one RPC) moves each old `remote/<id>/` mirror into a
  `.rejoin-backup-` sibling and un-registers it; `workspace prune --repo-mirrors` drives it, and
  `doctor` has a `mirror` row per mirror. `workspace bind` still works this release and says on stderr
  that it is going.
- **Decision 6.** A read or a write in a git root that holds a list registers it (CLI
  `register_guard::is_repo_root`; MCP `serves_here`). A plain unregistered folder still refuses a read
  and falls back to the default with a note.
- **Decision 7.** `merge3` keeps, per distinct line, the base copies less what either side deleted
  plus what either side added, so the *set* of lines is the same whichever side is ours. The *order*
  follows ours, not "the same either way round" as first thought. The driver is
  `txtodo merge-driver`; `txtodo git setup` writes `.gitattributes`, the clone's `merge.txtodo.driver`
  and `.txtodo/` in `.git/info/exclude`; `doctor` has a `git` row per clone without it.
- **Decision 7, automated (owner's request).** The person runs no git step. The daemon readies every
  repo it finds or registers by editing files inside the clone's own `.git`: `.txtodo/` in
  `info/exclude`, the list attributes in `info/attributes` (not the tracked `.gitattributes`, so no
  uncommitted change appears in a working tree) and the merge driver in `config`, as the plain
  command `txtodo merge-driver`, found on `PATH` when git runs it. `txtodo git setup` stays for a team
  that wants `.gitattributes` committed. The scan runs 5 s after the daemon starts, every 6 hours, on
  `txtodo repos scan`, and when the TUI starts or the desktop connects.
- **Decision 8.** The scan only registers (it opens nothing), never re-registers a folder the person
  removed, and seeds `[$HOME]` as the one default root only when no environment variable moved the
  daemon's socket or registry. Roots and the last scan time live in two tables of `registry.db` made
  `IF NOT EXISTS`, so the schema version did not move. `txtodo repos` lists, scans and edits roots.
  The cost check (RSS and boot time with many repos registered) has not been done.
- **Tests.** The two-daemon harness serves each test workspace as a list (`$TXTODO_LISTS_DIR` for the
  `--dir` bridge, `list_dir_in` in global mode); no hidden daemon flag was added. Removed with the
  behaviour they proved: the foreign-default and joiner-gains-a-folder e2e tests, and the bind e2e.
- **Known gaps.** The file carrier is still ungated. A list's name still rides offers to a foreign
  peer in the group (it has no route there). Mirrors an older build made stay registered until
  `prune --repo-mirrors`. Only a repo's root `todo.txt` is compared by `doctor`. A tagged-identity
  daemon would still stamp `id:` into a repo list on its first read.
