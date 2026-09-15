-- Device-set identity (ADR 0021, task daemon-device-set-identity): one device id, sync group and
-- keystore per *device*, not per workspace -- a separate database file from any workspace's own
-- oplog.db (this crate's main Store) and from the workspace registry's registry.db, since it lives
-- outside any single workspace and outside the catalog of directories too (txtodo-daemon resolves
-- the device-global path this file lives at; see that crate's device_identity.rs).
-- `meta` mirrors Store's own meta table exactly (device_id/group_id/group_key_epoch, the same
-- key/value shape) so txtodo-daemon's existing load-or-mint helpers need no new codec.
-- `devices` mirrors Store's own devices table exactly (migrations/0006.sql) -- known sync-group
-- peers are now a device-wide list, not duplicated per workspace; see that migration's own doc for
-- every column's meaning, unchanged here.
-- https://www.sqlite.org/lang_createtable.html · https://www.sqlite.org/pragma.html#pragma_user_version
CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB);
CREATE TABLE devices (
    device            BLOB    PRIMARY KEY,
    name              TEXT    NOT NULL,
    static_public     BLOB    NOT NULL,
    paired_at         INTEGER NOT NULL,
    last_seen         INTEGER,
    last_known_wall   INTEGER,
    key_epoch         INTEGER NOT NULL DEFAULT 0,
    removed_at        INTEGER
);
PRAGMA user_version = 1;
