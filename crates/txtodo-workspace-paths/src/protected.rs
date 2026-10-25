//! The folders macOS asks the person's permission for before a program may read them (task
//! `two-kinds-of-workspace`). Listing `~/Documents`, `~/Desktop` or `~/Downloads` from a program
//! that was not given access raises a system prompt naming that program, so a repo scan that walks
//! the home folder must not enter them on its own: the person links the ones that hold repos
//! (`txtodo repos roots add ~/Documents`) and answers the prompt once, knowing why.
//!
//! macOS has no call that says whether a folder is gated without reading it, and the read is what
//! raises the prompt, so this is a list of names. Other systems gate nothing: a list of none.
//! Ref: <https://support.apple.com/guide/mac-help/control-access-to-files-and-folders-on-mac-mchld5a35146/mac>

use std::path::{Path, PathBuf};

/// The folders directly under the home folder that macOS gates: the three that hold documents, and
/// the three media libraries (Photos and Music keep theirs inside `Pictures` and `Music`).
pub const MACOS_PROTECTED_HOME_FOLDERS: [&str; 6] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
];

/// [`MACOS_PROTECTED_HOME_FOLDERS`] under `home`, on any system. For tests; use
/// [`protected_home_folders`].
pub fn all_protected_under(home: &Path) -> Vec<PathBuf> {
    MACOS_PROTECTED_HOME_FOLDERS
        .iter()
        .map(|name| home.join(name))
        .collect()
}

/// The folders a scan of `home` must not enter unless the person linked them: the gated ones on
/// macOS, none elsewhere.
pub fn protected_home_folders(home: &Path) -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        all_protected_under(home)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gated_folders_sit_directly_under_home() {
        let all = all_protected_under(Path::new("/Users/a"));
        assert_eq!(all.len(), 6);
        assert!(all.contains(&PathBuf::from("/Users/a/Documents")));
        assert!(all.contains(&PathBuf::from("/Users/a/Downloads")));
        assert!(
            all.iter()
                .all(|p| p.parent() == Some(Path::new("/Users/a")))
        );
    }

    #[test]
    fn only_macos_gates_anything() {
        let gated = protected_home_folders(Path::new("/home/a"));
        assert_eq!(!gated.is_empty(), cfg!(target_os = "macos"));
    }
}
