//! Tauri's own build step: embeds `tauri.conf.json` and writes the capability schemas under
//! `gen/schemas/` for editor autocompletion. Ref: https://v2.tauri.app/reference/config/
fn main() {
    tauri_build::build()
}
