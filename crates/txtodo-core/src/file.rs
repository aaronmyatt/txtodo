//! Whole-file model: owned lines plus the hygiene that must round-trip (BOM, endings, trailing newline).
//! Design §2.2 rules 6 and 7: blank lines are entries; endings, BOM and trailing newline are preserved.

use crate::{Line, LineEnding, Mode, Quirks, parse_line};
use alloc::vec::Vec;

/// UTF-8 byte order mark.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

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
        debug_assert!(
            !bytes.contains(&b'\n'),
            "a line never contains its own ending"
        );
        OwnedLine {
            bytes,
            ending,
            quirks,
        }
    }
    /// Wraps line bytes and records the lenient parser's quirks (none for an opaque, non-UTF-8 line).
    pub fn from_bytes(bytes: Vec<u8>, ending: LineEnding) -> OwnedLine {
        let quirks = core::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| parse_line(s, Mode::Lenient).ok())
            .map_or(Quirks::NONE, |l| l.quirks);
        OwnedLine::new(bytes, ending, quirks)
    }
    /// The line bytes, minus ending.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// The line as text, or `None` when the bytes are not UTF-8 (an opaque line).
    pub fn raw(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes).ok()
    }
    /// Parses the line leniently; `None` for an opaque line. The returned ending is this line's real one.
    pub fn parse(&self) -> Option<Line<'_>> {
        let mut line = parse_line(self.raw()?, Mode::Lenient).ok()?;
        line.ending = self.ending;
        line.quirks = self.quirks;
        Some(line)
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

/// Splits raw file bytes into lines, detecting BOM, per-line endings, the dominant ending and the trailing
/// newline. Total over `&[u8]`: never fails, never loses a byte (`to_bytes` reproduces the input).
pub fn parse_file(bytes: &[u8]) -> File {
    let bom = bytes.starts_with(BOM);
    let body = if bom { &bytes[BOM.len()..] } else { bytes };
    let trailing_newline = body.last() == Some(&b'\n');
    let mut lines: Vec<OwnedLine> = split_lines(body)
        .map(|(b, e)| OwnedLine::from_bytes(b.to_vec(), e))
        .collect();
    let ending = dominant_ending(&lines);
    for line in lines
        .iter_mut()
        .filter(|l| l.ending != LineEnding::None && l.ending != ending)
    {
        line.quirks.insert(Quirks::MIXED_ENDING);
    }
    debug_assert!(
        lines
            .iter()
            .filter(|l| l.ending == LineEnding::None)
            .count()
            <= 1,
        "only the last line may lack a newline"
    );
    debug_assert!(
        trailing_newline == lines.last().is_some_and(|l| l.ending != LineEnding::None),
        "trailing newline agrees with the last line"
    );
    File {
        lines,
        bom,
        ending,
        trailing_newline,
    }
}

/// Yields `(line bytes, ending)` for each line. A final piece with no newline is a line with `None`;
/// a body ending in `\n` yields no extra empty line.
fn split_lines(body: &[u8]) -> impl Iterator<Item = (&[u8], LineEnding)> {
    let mut rest = body;
    core::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(nl) = rest.iter().position(|&b| b == b'\n') else {
            let last = rest;
            rest = &[];
            return Some((last, LineEnding::None));
        };
        let (line, after) = rest.split_at(nl);
        rest = &after[1..];
        let (line, ending) = match line.strip_suffix(b"\r") {
            Some(stripped) => (stripped, LineEnding::CrLf),
            None => (line, LineEnding::Lf),
        };
        debug_assert!(!line.contains(&b'\n'), "split at the first newline");
        Some((line, ending))
    })
}

/// Majority of `Lf` vs `CrLf`; ties and empty files are `Lf`.
fn dominant_ending(lines: &[OwnedLine]) -> LineEnding {
    let crlf = lines
        .iter()
        .filter(|l| l.ending == LineEnding::CrLf)
        .count();
    let lf = lines.iter().filter(|l| l.ending == LineEnding::Lf).count();
    debug_assert!(crlf + lf <= lines.len(), "counts cover at most every line");
    if crlf > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

impl File {
    /// The exact bytes: BOM, then every line with its own ending. Identity on an unchanged parse.
    pub fn to_bytes(&self) -> Vec<u8> {
        let size = self.lines.iter().map(|l| l.bytes.len() + 2).sum::<usize>() + BOM.len();
        let mut out = Vec::with_capacity(size);
        if self.bom {
            out.extend_from_slice(BOM);
        }
        for line in &self.lines {
            out.extend_from_slice(&line.bytes);
            out.extend_from_slice(line.ending.as_bytes());
        }
        debug_assert!(out.len() <= size, "capacity estimate is an upper bound");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_line_reports_invalid_utf8_as_opaque() {
        let line = OwnedLine::from_bytes(alloc::vec![0xFF, b'x'], LineEnding::Lf);
        assert_eq!(
            (line.raw(), line.parse(), line.quirks()),
            (None, None, Quirks::NONE)
        );
        assert_eq!(line.bytes(), &[0xFF, b'x']);
    }

    #[test]
    fn hygiene_files_round_trip_and_are_described() {
        for (name, bytes) in [
            ("crlf", &include_bytes!("../../../corpus/crlf.txt")[..]),
            ("bom", include_bytes!("../../../corpus/bom.txt")),
            (
                "no-trailing-newline",
                include_bytes!("../../../corpus/no-trailing-newline.txt"),
            ),
            (
                "mixed-endings",
                include_bytes!("../../../corpus/mixed-endings.txt"),
            ),
            ("empty", b""),
            ("lone newline", b"\n"),
        ] {
            assert_eq!(parse_file(bytes).to_bytes(), bytes, "{name}");
        }
        let crlf = parse_file(include_bytes!("../../../corpus/crlf.txt"));
        assert_eq!(
            (crlf.lines.len(), crlf.ending, crlf.trailing_newline),
            (3, LineEnding::CrLf, true)
        );
        assert!(
            crlf.lines
                .iter()
                .all(|l| l.ending() == LineEnding::CrLf && !l.quirks().has(Quirks::MIXED_ENDING))
        );
        let bom = parse_file(include_bytes!("../../../corpus/bom.txt"));
        assert!(bom.bom && bom.lines[0].raw() == Some("2026-09-11 File starts with a BOM +bom"));
        let ntn = parse_file(include_bytes!("../../../corpus/no-trailing-newline.txt"));
        assert_eq!(
            (ntn.trailing_newline, ntn.lines[1].ending()),
            (false, LineEnding::None)
        );
        let mixed = parse_file(include_bytes!("../../../corpus/mixed-endings.txt"));
        assert_eq!(mixed.ending, LineEnding::Lf);
        assert_eq!(
            mixed
                .lines
                .iter()
                .map(|l| l.quirks().has(Quirks::MIXED_ENDING))
                .collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(
            parse_file(b"\n").lines.len(),
            1,
            "a lone newline is one blank entry"
        );
    }
}
