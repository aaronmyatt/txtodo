-- needs_review flags and the Loro mirror snapshot (plan M4, tasks/crdt-needs-review). Both are
-- local, additive and rebuildable; neither touches the op log or the projection bytes.
--   review_flags: one open or cleared flag per (file, task); mine/theirs are the description bytes
--   each side had when the flag was raised (stored, not recomputed). Clearing is an upsert of
--   cleared_at, so the crate still has no UPDATE statement of its own.
--   mirrors: the daemon's Loro document snapshot per file at log position seq, so two devices keep
--   shared lineage across restarts (ops after seq are replayed into it on open).
-- https://www.sqlite.org/lang_upsert.html · https://www.sqlite.org/pragma.html#pragma_user_version
CREATE TABLE review_flags (
    file       TEXT    NOT NULL,
    task       BLOB    NOT NULL,
    raised_at  INTEGER NOT NULL,
    mine       BLOB    NOT NULL,
    theirs     BLOB    NOT NULL,
    cleared_at INTEGER,
    PRIMARY KEY (file, task)
);
CREATE TABLE mirrors (file TEXT PRIMARY KEY, snapshot BLOB NOT NULL, seq INTEGER NOT NULL);
PRAGMA user_version = 3;
