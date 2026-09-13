// Frontend-only display config (tasks/desktop-main-view). Not wired to the daemon's own
// `config.toml` (that governs `todo_dir`/`id_tags`/`url_schemes` on the Rust side) — this is
// purely "how this app chooses to render what the daemon sends", so a plain module constant is
// honest about scope; promote it to a real settings file/UI only once something else needs to
// change it at runtime.
export const showIdTags = false;
