//! `txtodo lists import DIR [NAME]` (task `two-kinds-of-workspace`, ADR 0043): copies a folder into
//! a new list. A folder that is not a git repo stops syncing through txtodo once repos leave txtodo
//! sync, and an old Remote mirror stops with it; this is the way back. The copy is of files, never
//! of the folder's `.txtodo/` op log, so the list starts with a fresh history and the daemon reads
//! the copied files as the first outside edit it sees.
//!
//! Refused when the list already exists (importing would mix two unrelated files into it) and when
//! the folder holds no `todo.txt`.

use std::path::{Path, PathBuf};

use crate::client::Daemon;
use crate::{CliError, json};

/// The longest list name (`txtodo-daemon`'s `list_workspace.rs`).
const MAX_NAME: usize = 48;

/// Runs the import.
pub fn run(
    daemon: &mut Daemon,
    dir: &str,
    name: Option<&str>,
    as_json: bool,
) -> Result<(), CliError> {
    let src = std::fs::canonicalize(dir)
        .map_err(|e| CliError::Message(format!("txtodo: cannot read {dir}: {e}")))?;
    if !src.join("todo.txt").is_file() {
        return Err(CliError::Message(format!(
            "txtodo: {} holds no todo.txt, so there is nothing to import",
            src.display()
        )));
    }
    let name = match name {
        Some(n) => n.to_owned(),
        None => suggest_name(&src).ok_or_else(|| {
            CliError::Message(format!(
                "txtodo: no list name can be made from {}; give one: `txtodo lists import {dir} NAME`",
                src.display()
            ))
        })?,
    };
    let made = daemon.list_ensure(&name)?;
    let info = made
        .workspace
        .ok_or_else(|| CliError::Message("txtodo: the daemon made no list".to_owned()))?;
    if !made.created {
        return Err(CliError::Message(format!(
            "txtodo: list {} already exists; import into a new name, or move the files by hand",
            info.name
        )));
    }
    let root = PathBuf::from(&info.root);
    let copied = copy_tree(&src, &root).map_err(|e| {
        CliError::Message(format!(
            "txtodo: list {} was made at {} but copying stopped: {e}",
            info.name,
            root.display()
        ))
    })?;
    // The folder's own `txtodo.toml` may carry a `name` line; the list shows the name it was given.
    if src.join("txtodo.toml").is_file() {
        daemon.list_rename(&info.name, &info.name)?;
    }
    if as_json {
        println!(
            r#"{{"name":{},"root":{},"files":{copied}}}"#,
            json::str(&info.name),
            json::str(&info.root)
        );
    } else {
        println!(
            "TODO: copied {copied} file(s) from {} into list {} at {}",
            src.display(),
            info.name,
            info.root
        );
    }
    Ok(())
}

/// A list name from a folder's name: lowercase, anything but a-z, 0-9, `-` and `_` turned into
/// `-`, cut to the longest name, starting with a letter or digit. `None` when nothing is left.
fn suggest_name(dir: &Path) -> Option<String> {
    let folder = dir.file_name()?.to_string_lossy().to_ascii_lowercase();
    let mapped: String = folder
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = mapped.trim_start_matches(['-', '_']);
    let cut: String = trimmed.chars().take(MAX_NAME).collect();
    (!cut.is_empty()).then_some(cut)
}

/// Copies every file under `from` into `to`, making folders as it goes: hidden entries (`.git`,
/// `.txtodo`, `.DS_Store`), symbolic links and anything that is not a plain file or folder are left
/// out, and so is `to` itself if it sits inside `from`. An existing file in `to` is overwritten
/// (the empty `todo.txt` a new list starts with). Returns how many files were copied.
/// Ref: <https://doc.rust-lang.org/std/fs/fn.copy.html>
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(to)?;
    let skip = std::fs::canonicalize(to).ok();
    let mut copied = 0;
    let mut stack = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        std::fs::create_dir_all(&dst_dir)?;
        for entry in std::fs::read_dir(&src_dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type()?;
            let (src, dst) = (entry.path(), dst_dir.join(entry.file_name()));
            if kind.is_dir() && skip.as_deref() != std::fs::canonicalize(&src).ok().as_deref() {
                stack.push((src, dst));
            } else if kind.is_file() {
                std::fs::copy(&src, &dst)?;
                copied += 1;
            }
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn a_folder_name_becomes_a_list_name() {
        let name = |p: &str| suggest_name(Path::new(p));
        assert_eq!(name("/home/a/My Notes").as_deref(), Some("my-notes"));
        assert_eq!(name("/x/Work_2026").as_deref(), Some("work_2026"));
        assert_eq!(name("/x/.hidden").as_deref(), Some("hidden"));
        assert_eq!(name("/x/---").as_deref(), None);
        let long = format!("/x/{}", "a".repeat(80));
        assert_eq!(name(&long).unwrap().len(), MAX_NAME);
    }

    #[test]
    fn the_copy_takes_files_and_folders_and_leaves_hidden_things_behind() {
        let src = tree(&[
            ("todo.txt", "buy milk\n"),
            ("notes.md", "n\n"),
            ("tasks/trip/todo.txt", "book flights\n"),
            (".txtodo/oplog.db", "binary"),
            (".git/config", "git"),
            (".DS_Store", "junk"),
        ]);
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(dst.path().join("todo.txt"), "").unwrap();

        let copied = copy_tree(src.path(), dst.path()).unwrap();

        assert_eq!(copied, 3);
        let read = |rel: &str| std::fs::read_to_string(dst.path().join(rel)).unwrap();
        assert_eq!(
            read("todo.txt"),
            "buy milk\n",
            "the empty seed is overwritten"
        );
        assert_eq!(read("tasks/trip/todo.txt"), "book flights\n");
        for left_out in [".txtodo", ".git", ".DS_Store"] {
            assert!(!dst.path().join(left_out).exists(), "{left_out}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_not_followed() {
        let src = tree(&[("todo.txt", "a\n")]);
        std::os::unix::fs::symlink("/etc", src.path().join("etc-link")).unwrap();
        let dst = tempfile::tempdir().unwrap();
        assert_eq!(copy_tree(src.path(), dst.path()).unwrap(), 1);
        assert!(!dst.path().join("etc-link").exists());
    }

    #[test]
    fn a_destination_inside_the_source_is_not_copied_into_itself() {
        let src = tree(&[("todo.txt", "a\n")]);
        let dst = src.path().join("lists-here");
        let copied = copy_tree(src.path(), &dst).unwrap();
        assert_eq!(copied, 1, "todo.txt only, not the new folder");
        assert!(!dst.join("lists-here").exists());
    }
}
