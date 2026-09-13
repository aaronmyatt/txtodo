-- Workspace registry (ADR 0025, task daemon-workspace-registry): the device-global catalog of
-- every todo directory this device's one txtodod knows about. A separate database file from any
-- single workspace's own oplog.db (ADR 0010 fixes that at <workspace>/.txtodo/oplog.db) -- this
-- one lives at a device-global location the daemon crate resolves (see
-- tasks/daemon-workspace-registry/notes.md for the exact path and why), with its own
-- PRAGMA user_version numbering independent of the op log's.
-- `id` is a minted ULID, never the path itself (paths get renamed/moved); `root` is the
-- workspace's canonicalized absolute path, stored only to detect "already registered", never
-- read back as identity. Removal follows this crate's tombstone idiom (`devices.rs`'s
-- `remove_device`): `removed_at` NULL means active, and a row is kept forever, never deleted --
-- the same "catalog change, not data destruction" discipline that removal must give the
-- workspace's own <workspace>/.txtodo/ state (this table never names that path at all).
-- https://www.sqlite.org/lang_createtable.html · https://www.sqlite.org/partialindex.html
CREATE TABLE workspaces (
    id          BLOB    PRIMARY KEY,
    root        TEXT    NOT NULL,
    added_at    INTEGER NOT NULL,
    removed_at  INTEGER
);
-- Only one *active* row per root; once removed, the same root is free to be registered again
-- (minting a fresh id -- see WorkspaceRegistry::add's doc in txtodo-daemon).
CREATE UNIQUE INDEX workspaces_root_active ON workspaces(root) WHERE removed_at IS NULL;
PRAGMA user_version = 1;
