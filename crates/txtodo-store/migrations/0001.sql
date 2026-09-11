-- txtodo op log, plan M3 (verbatim DDL) + pragmas. Applied once, tracked by PRAGMA user_version.
-- https://www.sqlite.org/wal.html · https://www.sqlite.org/pragma.html#pragma_user_version
CREATE TABLE ops (
    seq         INTEGER PRIMARY KEY,
    op_id       BLOB UNIQUE,
    hlc_wall    INTEGER,
    hlc_counter INTEGER,
    device      BLOB,
    principal   TEXT,
    file        TEXT,
    kind        TEXT,
    payload     BLOB,
    signature   BLOB
);
CREATE INDEX ops_file_hlc ON ops(file, hlc_wall, hlc_counter);
CREATE TABLE projections (file TEXT PRIMARY KEY, bytes BLOB, hash BLOB, written_at INTEGER);
CREATE TABLE snapshots (file TEXT, seq INTEGER, state BLOB, PRIMARY KEY(file, seq));
CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB);   -- device id, keys (encrypted), schema version
PRAGMA user_version = 1;
