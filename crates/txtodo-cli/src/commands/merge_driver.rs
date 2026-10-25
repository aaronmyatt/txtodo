//! `txtodo merge-driver %O %A %B` (task `two-kinds-of-workspace`, ADR 0043): the merge git runs on a
//! repo's `todo.txt` when both sides changed it. It reads the ancestor, ours and theirs, merges them
//! three ways line by line with `txtodo_core::merge3` (which never loses a line), and writes the
//! result over ours, the file git reads the answer from. Exit 0 says it merged.
//!
//! No workspace, config or daemon is involved: git calls it with three temp files, possibly while
//! the daemon watches the working tree. A file that is not UTF-8 is left alone with exit 1, which
//! git reads as a conflict it asks the user to settle, as it would with no driver.
//! Ref: <https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver>

use std::path::Path;

use crate::CliError;

/// Merges `theirs` into `ours` against `base`; the paths are git's `%O`, `%A` and `%B`.
pub fn run(base: &str, ours: &str, theirs: &str) -> Result<(), CliError> {
    let merged = txtodo_core::merge3(&text(base)?, &text(ours)?, &text(theirs)?);
    std::fs::write(ours, merged)
        .map_err(|e| CliError::Message(format!("txtodo: merge-driver: cannot write {ours}: {e}")))
}

/// The file as text. A missing file is an empty one (git passes an empty temp file for an
/// ancestor that did not exist); one that is not UTF-8 is refused.
fn text(path: &str) -> Result<String, CliError> {
    match std::fs::read(Path::new(path)) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            CliError::Message(format!(
                "txtodo: merge-driver: {path} is not UTF-8; leaving the merge to git"
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(CliError::Message(format!(
            "txtodo: merge-driver: cannot read {path}: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(base: &str, ours: &str, theirs: &str) -> (tempfile::TempDir, [String; 3]) {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str, body: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, body).unwrap();
            p.to_string_lossy().into_owned()
        };
        let paths = [
            path("base", base),
            path("ours", ours),
            path("theirs", theirs),
        ];
        (dir, paths)
    }

    #[test]
    fn the_merge_is_written_over_ours() {
        let (_dir, [base, ours, theirs]) = files("a\nb\n", "a\nb\nours\n", "a\nb\ntheirs\n");
        run(&base, &ours, &theirs).unwrap();
        assert_eq!(
            std::fs::read_to_string(&ours).unwrap(),
            "a\nb\ntheirs\nours\n"
        );
        assert_eq!(
            std::fs::read_to_string(&theirs).unwrap(),
            "a\nb\ntheirs\n",
            "theirs is read only"
        );
    }

    #[test]
    fn a_missing_ancestor_is_an_empty_one() {
        let (dir, [base, ours, theirs]) = files("", "one\n", "two\n");
        std::fs::remove_file(&base).unwrap();
        run(&base, &ours, &theirs).unwrap();
        let merged = std::fs::read_to_string(&ours).unwrap();
        assert!(merged.contains("one") && merged.contains("two"), "{merged}");
        drop(dir);
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused_and_left_alone() {
        let (_dir, [base, ours, theirs]) = files("a\n", "a\n", "a\n");
        std::fs::write(&ours, [0xff, 0xfe, b'\n']).unwrap();
        let why = run(&base, &ours, &theirs).unwrap_err().to_string();
        assert!(why.contains("not UTF-8"), "{why}");
        assert_eq!(std::fs::read(&ours).unwrap(), [0xff, 0xfe, b'\n']);
    }
}
