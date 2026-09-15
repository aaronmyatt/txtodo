-- Mirrors migrations/0007.sql for the device-global devices table -- see that migration's own doc
-- for what these columns mean; unchanged here.
-- https://www.sqlite.org/lang_altertable.html
ALTER TABLE devices ADD COLUMN relay_node_id BLOB;
ALTER TABLE devices ADD COLUMN relay_url TEXT;
PRAGMA user_version = 2;
