-- Task default-workspace-pairing-consent (decided 2026-09-24): whether both sides called the other
-- their own device at pairing. The default workspace merges only with such a peer. Every device
-- already paired counts as own (the decided migration: pairing a device that is not yours was never
-- a supported flow), so the column defaults to 1 for existing rows.
-- https://www.sqlite.org/lang_altertable.html
ALTER TABLE devices ADD COLUMN own_device INTEGER NOT NULL DEFAULT 1;
PRAGMA user_version = 3;
