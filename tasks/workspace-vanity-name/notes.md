# workspace-vanity-name

## Goal
A workspace synced between paired devices shows one human name on every device. Before this, the
remote device showed the mirror's folder, `remote/<ULID>`, which tells a person nothing. The name
is display only: ids stay ULIDs, and nothing routes or matches by it.

## Design

### Set where
- `txtodo workspace rename <id> <name>` → `WorkspaceRename { workspace_id, name }`, device-level
  like `WorkspaceRemove`. Returns the `WorkspaceInfo`. An empty name clears it.
- A name is trimmed, at most 256 bytes (`MAX_WORKSPACE_NAME_BYTES`, what an offer carries), with
  no control characters.
- With no name set: `default` for the default workspace, else the folder's name.

### Stored where
- A top-level `name = "..."` line in `<root>/txtodo.toml`. That file already holds the layout and
  already syncs as a whole-text document (`layout_sync.rs`: a notes actor under the path
  `txtodo.toml`). So no store table, no registry column, no new wire field.
- An edit touches that one line; every other byte stays. A layout change keeps the name.
- Rejected: a registry column (a store change, and it would not sync by itself). A new field on
  the control-channel `Offer` (postcard, not self-describing: an old peer fails to decode it).

### Travels how
- The file's bytes travel as `NotesEdit` ops, the op kind notes.md already uses. Sync frames are
  postcard, but nothing new is in them: an existing op kind carrying new text.
- Until the file arrives, or when nobody named it, a mirror shows what its offering device calls
  it. `ControlMessage::Offer.name` already existed (postcard, unchanged). It used to be the folder's
  basename; now it is the offering device's shown name. The receiver keeps the last one per device
  in `WorkspaceOfferRegistry` (memory only). A device offers a mirror under its shown name too, so
  a third device learns the name, not the id.
- Daemon ↔ clients: `WorkspaceInfo.name` (field 12) and the `WorkspaceRename` rpc. Additive
  protobuf.

### Conflict rule
- The file merges like notes.md: Loro text, character by character. No clock decides.
- A rename is two ops: the old line out, then the new line in (`rename_steps`). Two renames at once
  on two devices merge into two whole `name` lines, never one name spliced into the other (one
  diff per rename would give `GroceriesShopping`). Tested on two Loro forks.
- Two `name` lines are invalid TOML. The name then reads the last top-level `name` line. Both
  devices hold the same text, so both show the same name. The layout parser skips `name` lines, so
  the layout still loads. The next rename writes one line again.
- So it is "one of the two, the same on every device", not "last writer by time". Loro orders the
  two inserts, not the wall clock.

### Shown where
- `WorkspaceList`/`WorkspaceAdd`/`WorkspaceAcceptOffer`/`WorkspaceRename` fill `WorkspaceInfo.name`.
  `UniversalTasks` rows use it as the workspace name.
- CLI: `workspace list` prints it after the id; JSON has `name`. `workspace offers` prints the
  offered name, which is now the shown name.
- TUI: W popup, Settings > Workspaces and the header (at start and after `:w`); `:w <name>` finds
  it.
- Desktop: the switcher shows it before the path when it differs from the folder's name (a mirror).

## As built
- daemon `66b6faf`: `workspace_name.rs` reads and edits the `name` line; the layout keeps it and
  reads past it.
- daemon `66285b1`: `WorkspaceInfo.name`; offered names kept per device; offers carry the shown
  name; Universal rows use it.
- proto `81acf5f`: the `WorkspaceRename` rpc (the messages were `bc8b1ed`). The daemon does not
  build between this and `af24605`, the same split as `WorkspaceRejoin`.
- daemon `af24605`: `WorkspaceRename` through the file's notes actor, two ops. A rename that makes
  the file writes the live layout lines too, so the new file cannot read as another layout.
- daemon `138e285`: `tests/workspace_name.rs`, two real daemons: B's mirror lists `plants` (A's
  offer); A renames and B lists the new name, with it in B's `txtodo.toml`; B's rename reaches A.
- cli `ab23c08`, desktop `d869b08`, and the tui commit that carries this file.
- Still broken / not done:
  - The offered name lives in memory. After a restart, a mirror whose owner never set a name shows
    its id until that owner's next offer (offers go out on every control session).
  - A foreign device's default mirror shows `default` until its owner names it. Easy to mix up
    with the device's own default.
  - Two renames at once keep one name picked by Loro's order, not by time. The file keeps two
    lines until the next rename.
  - Renaming a mirror before its `txtodo.toml` has arrived starts a second lineage of that file.
    The merge can then hold both sides' layout lines. Same gap as a notes.md made on two devices.
  - `name` in a form the one-line edit does not see (a quoted key, a multi-line string) is refused
    with FAILED_PRECONDITION. Fix it by hand.
  - Only the CLI sets a name. No rename in the TUI or desktop yet. MCP's workspace list does not
    carry the name.
