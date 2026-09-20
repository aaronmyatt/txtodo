-- Task op-source: which client made a change ("cli", "tui", "desktop", "mcp"), or "sync" for an op
-- that arrived from another device and "external" for an edit seen on disk. Local to this device's
-- op log: it is never in the postcard payload, so it is not in the op's hash or signature and never
-- syncs. NULL for a row written before this migration, or by a caller that named no source.
-- https://www.sqlite.org/lang_altertable.html
ALTER TABLE ops ADD COLUMN source TEXT;
PRAGMA user_version = 8;
