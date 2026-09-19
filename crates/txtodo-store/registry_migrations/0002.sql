-- last_active_ms (task daemon-early-bind): when a workspace was last resolved for a request, in
-- unix milliseconds, so a cold txtodod can open the most recently used workspaces first. Additive
-- and nullable: a row that was never touched (or predates this column) is NULL, and the daemon
-- falls back to the root todo.txt's mtime, then to added_at.
-- https://www.sqlite.org/lang_altertable.html#altertabaddcol
ALTER TABLE workspaces ADD COLUMN last_active_ms INTEGER;
PRAGMA user_version = 2;
