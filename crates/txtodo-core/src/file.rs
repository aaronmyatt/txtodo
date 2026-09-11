//! Whole-file model: owned lines plus the hygiene that must round-trip (BOM, endings, trailing newline).

use crate::{LineEnding, Quirks};
use alloc::vec::Vec;

/// An owned line as stored in a [`File`]: the exact bytes, its ending, and the quirks seen when it was read.
/// Bytes that are not valid UTF-8 are kept verbatim and reported by lint, never dropped (design §4.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedLine {
    bytes: Vec<u8>,
    ending: LineEnding,
    quirks: Quirks,
}

impl OwnedLine {
    /// Wraps line bytes (without ending). Callers pass what they read; nothing is validated here.
    pub fn new(bytes: Vec<u8>, ending: LineEnding, quirks: Quirks) -> OwnedLine {
        debug_assert!(!bytes.contains(&b'\n'), "a line never contains its own ending");
        OwnedLine { bytes, ending, quirks }
    }
    /// The line bytes, minus ending.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// The line as text, or `None` when the bytes are not UTF-8 (an opaque line).
    pub fn raw(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes).ok()
    }
    /// How this line ends.
    pub fn ending(&self) -> LineEnding {
        self.ending
    }
    /// Quirks recorded when the line was read.
    pub fn quirks(&self) -> Quirks {
        self.quirks
    }
}

/// A whole todo.txt file: lines plus the hygiene that must round-trip (BOM, dominant ending, trailing newline).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct File {
    /// Every entry, blank lines included, in file order.
    pub lines: Vec<OwnedLine>,
    /// The file started with a UTF-8 byte order mark.
    pub bom: bool,
    /// The dominant ending; new lines appended by tools use it. Each line still keeps its own.
    pub ending: LineEnding,
    /// The last line ended with a newline.
    pub trailing_newline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_line_reports_invalid_utf8_as_opaque() {
        let line = OwnedLine::new(alloc::vec![0xFF, b'x'], LineEnding::Lf, Quirks::NONE);
        assert_eq!(line.raw(), None);
        assert_eq!(line.bytes(), &[0xFF, b'x']);
    }
}
