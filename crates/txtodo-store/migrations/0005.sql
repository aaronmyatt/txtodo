-- Sidecar identity fingerprints (design §4.1, docs/questions.md Q2, now the default identity
-- mode): one row per task the last time its fingerprint was computed, so an external edit can be
-- re-matched by solving an assignment problem (crates/txtodo-daemon/src/identity/assign.rs)
-- instead of reading an `id:` tag. `projects`/`contexts` are postcard of a sorted `Vec<String>`
-- (this crate's one payload codec, same idiom as `tokens.scopes`); `creation_date` is
-- `year*10000 + month*100 + day`, NULL when the task has none. Retiring follows the crate's
-- upsert idiom (`flags.rs`'s `clear_flag`): a tombstoned row is kept, never deleted, so a
-- late-arriving peer op against a task since split by a delete+insert still finds a row that
-- explains it. `status` duplicates what `retired_at IS NULL` already says, kept as its own column
-- so a lookup can filter on it directly without a NULL check.
-- https://www.sqlite.org/lang_createtable.html · https://www.sqlite.org/pragma.html#pragma_user_version
CREATE TABLE fingerprints (
    file              TEXT NOT NULL,
    task              BLOB NOT NULL,
    status            TEXT NOT NULL DEFAULT 'live',
    creation_date     INTEGER,
    projects          BLOB NOT NULL,
    contexts          BLOB NOT NULL,
    description_norm  TEXT NOT NULL,
    line_index        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    retired_at        INTEGER,
    PRIMARY KEY (file, task)
);
CREATE INDEX fingerprints_file_status ON fingerprints(file, status);
PRAGMA user_version = 5;
