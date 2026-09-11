//! Direct-file mode I/O: read a whole file into a [`File`], write it back atomically (temp + rename).
//! Bytes txtodo did not change come back identical (design §2.2 rules 2 and 7).

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use txtodo_core::{File, LineEnding, OwnedLine, parse_file};

/// A filesystem operation that failed, with what was attempted and on which path.
#[derive(Debug)]
pub struct StoreError {
    op: &'static str,
    path: PathBuf,
    source: io::Error,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot {} {}: {}",
            self.op,
            self.path.display(),
            self.source
        )
    }
}

fn err(op: &'static str, path: &Path) -> impl FnOnce(io::Error) -> StoreError {
    let path = path.to_path_buf();
    move |source| StoreError { op, path, source }
}

/// Reads and splits the file. A missing file is an empty [`File`] (todo.sh creates on first write).
pub fn read(path: &Path) -> Result<File, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(parse_file(&bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(File::default()),
        Err(e) => Err(err("read", path)(e)),
    }
}

/// Writes `file` to a temp file beside `path`, fsyncs, then renames over `path`. Readers see the old
/// bytes or the new bytes, never a prefix. https://doc.rust-lang.org/std/fs/fn.rename.html
pub fn write(path: &Path, file: &File) -> Result<(), StoreError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.txtodo-{}.tmp", std::process::id()));
    let bytes = file.to_bytes();
    debug_assert!(
        tmp.parent() == path.parent(),
        "temp file shares the directory"
    );
    let result = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(&bytes).and_then(|()| f.sync_all()))
        .map_err(err("write", &tmp))
        .and_then(|()| std::fs::rename(&tmp, path).map_err(err("replace", path)));
    if result.is_err() {
        // Best effort: the error being reported is the one that matters.
        let _ = std::fs::remove_file(&tmp);
    }
    debug_assert!(
        result.is_err() || !tmp.exists(),
        "temp file consumed by the rename"
    );
    result
}

/// Appends one line using the file's dominant ending. A last line without a newline is terminated
/// first (todo.sh `fixMissingEndOfLine`), so the new line never glues onto it. Returns its 1-based number.
pub fn append_line(file: &mut File, bytes: Vec<u8>) -> usize {
    let ending = file.ending;
    debug_assert!(ending != LineEnding::None, "dominant ending is Lf or CrLf");
    if let Some(last) = file.lines.last_mut()
        && last.ending() == LineEnding::None
    {
        *last = OwnedLine::new(last.bytes().to_vec(), ending, last.quirks());
    }
    file.lines.push(OwnedLine::from_bytes(bytes, ending));
    file.trailing_newline = true;
    debug_assert!(
        file.lines.iter().all(|l| l.ending() != LineEnding::None),
        "every line terminated"
    );
    file.lines.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_terminates_a_dangling_last_line_with_the_dominant_ending() {
        let mut crlf = parse_file(b"a\r\nb");
        assert_eq!(append_line(&mut crlf, b"c".to_vec()), 3);
        assert_eq!(crlf.to_bytes(), b"a\r\nb\r\nc\r\n");
        let mut empty = File::default();
        assert_eq!(append_line(&mut empty, b"x".to_vec()), 1);
        assert_eq!(empty.to_bytes(), b"x\n");
    }

    #[test]
    fn write_then_read_round_trips_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("todo.txt");
        let file = parse_file(b"\xEF\xBB\xBFone\r\ntwo\r\n");
        write(&path, &file).unwrap();
        assert_eq!(read(&path).unwrap(), file);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(read(&dir.path().join("none.txt")).unwrap(), File::default());
    }
}
