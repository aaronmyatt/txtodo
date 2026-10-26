//! Images dropped or pasted into a task's notes (task enable-dragging-and-dropping-images-to-a).
//! Decided 2026-10-11 (option A in that task's notes.md): the app writes the image itself, beside
//! the task's notes.md as `assets/<hash>-<name>`, and the notes link it with plain markdown
//! (`![name](assets/…)`), so the image reads in any editor. Local to this device: no sync carries
//! the bytes yet (option B, a daemon blob path, stays its own ADR). Both commands only ever touch
//! an `assets/` folder beside a `notes.md` inside the open workspace.
//! Ref: https://doc.rust-lang.org/std/fs/fn.write.html

use std::path::{Component, Path, PathBuf};

use tauri::State;

use crate::state::AppState;

/// The largest image accepted, so a stray drop of a huge file does not land in the workspace.
const MAX_BYTES: usize = 10 * 1024 * 1024;
/// Image types the notes show. No SVG: it is a document that can carry script.
const KINDS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// A workspace-relative path with no `..`, no root and nothing odd: the only shape accepted.
fn is_plain_relative(p: &Path) -> bool {
    p.components().all(|c| matches!(c, Component::Normal(_)))
}

/// The `assets/` folder beside `notes_path` (workspace-relative, ending in `notes.md`).
fn assets_dir(root: &Path, notes_path: &str) -> Result<PathBuf, String> {
    let notes = Path::new(notes_path);
    if !is_plain_relative(notes) || notes.file_name().is_none_or(|n| n != "notes.md") {
        return Err(format!("not a notes.md path: {notes_path}"));
    }
    let dir = notes.parent().unwrap_or(Path::new(""));
    Ok(root.join(dir).join("assets"))
}

/// The file name kept: `[A-Za-z0-9._-]` only (others become `-`), and a known image extension.
fn safe_name(name: &str) -> Result<String, String> {
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image");
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let ext = Path::new(&clean)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !KINDS.contains(&ext.as_str()) {
        return Err(format!("not an image this app shows: {base}"));
    }
    Ok(clean)
}

/// FNV-1a 64 of `bytes`: a stable short prefix so two different images with one name never
/// overwrite each other, and the same image dropped twice is one file. Not a security hash.
/// Ref: https://datatracker.ietf.org/doc/html/draft-eastlake-fnv-17
fn short_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", h >> 32)
}

async fn root(state: &AppState) -> Result<PathBuf, String> {
    state
        .current_workspace
        .lock()
        .await
        .clone()
        .ok_or_else(|| "no workspace open".to_owned())
}

/// Writes `bytes` as an image beside `notes_path` under `root`; returns `assets/<hash>-<name>`.
/// `pub` for the e2e bridge, which serves the same command over HTTP.
pub fn save_asset(
    root: &Path,
    notes_path: &str,
    name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("image over {} MB", MAX_BYTES / 1024 / 1024));
    }
    let dir = assets_dir(root, notes_path)?;
    let file = format!("{}-{}", short_hash(bytes), safe_name(name)?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&file), bytes).map_err(|e| e.to_string())?;
    Ok(format!("assets/{file}"))
}

/// The bytes of `link` (`assets/…`, beside `notes_path` under `root`). `pub` for the e2e bridge.
pub fn read_asset(root: &Path, notes_path: &str, link: &str) -> Result<Vec<u8>, String> {
    let rel = Path::new(link);
    let inside = rel
        .strip_prefix("assets")
        .map_err(|_| format!("not an assets/ link: {link}"))?;
    if !is_plain_relative(rel) || inside.as_os_str().is_empty() {
        return Err(format!("not an assets/ link: {link}"));
    }
    safe_name(link)?;
    let path = assets_dir(root, notes_path)?.join(inside);
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err(format!("image over {} MB", MAX_BYTES / 1024 / 1024));
    }
    Ok(bytes)
}

/// Writes one dropped image beside `notes_path`; returns the link target, `assets/<hash>-<name>`.
#[tracing::instrument(name = "ipc.save_note_asset", skip_all)]
#[tauri::command]
pub async fn save_note_asset(
    state: State<'_, AppState>,
    notes_path: String,
    name: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    save_asset(&root(&state).await?, &notes_path, &name, &bytes)
}

/// Reads an image the notes link (`assets/…`, relative to `notes_path`'s folder).
#[tracing::instrument(name = "ipc.read_note_asset", skip_all)]
#[tauri::command]
pub async fn read_note_asset(
    state: State<'_, AppState>,
    notes_path: String,
    link: String,
) -> Result<Vec<u8>, String> {
    read_asset(&root(&state).await?, &notes_path, &link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_notes_md_inside_the_workspace_has_an_assets_folder() {
        let root = Path::new("/w");
        assert_eq!(
            assets_dir(root, "tasks/trip/notes.md").unwrap_or_default(),
            PathBuf::from("/w/tasks/trip/assets")
        );
        assert!(assets_dir(root, "../x/notes.md").is_err());
        assert!(assets_dir(root, "/etc/notes.md").is_err());
        assert!(assets_dir(root, "tasks/trip/todo.txt").is_err());
    }

    #[test]
    fn names_are_cleaned_and_must_be_images() {
        assert_eq!(
            safe_name("my photo (1).PNG").as_deref(),
            Ok("my-photo--1-.PNG")
        );
        assert_eq!(safe_name("../../evil.jpg").as_deref(), Ok("evil.jpg"));
        assert!(safe_name("drawing.svg").is_err(), "SVG can carry script");
        assert!(safe_name("notes.txt").is_err());
    }

    #[test]
    fn the_hash_is_stable_and_short() {
        assert_eq!(short_hash(b"abc"), short_hash(b"abc"));
        assert_ne!(short_hash(b"abc"), short_hash(b"abd"));
        assert_eq!(short_hash(b"").len(), 8);
    }
}
