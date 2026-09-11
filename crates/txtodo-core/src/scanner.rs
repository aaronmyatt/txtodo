//! The one scanner both the parser and the tokenizer use: a line is alternating runs of whitespace and words.
//! Only ASCII space and tab split words (ABNF `SP`; tabs are a lenient quirk). Unicode spaces are `NONSP`.

/// A run of bytes: whitespace or a word. Offsets are byte offsets into the scanned line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Start byte, inclusive.
    pub start: usize,
    /// End byte, exclusive.
    pub end: usize,
    /// Spaces and/or tabs.
    pub is_ws: bool,
}

/// True for the two separators the grammar knows.
pub(crate) fn is_ws(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// Splits `raw` into contiguous chunks covering every byte. Whitespace bytes are ASCII, so every chunk
/// boundary is a UTF-8 char boundary.
pub(crate) fn chunks(raw: &str) -> impl Iterator<Item = Chunk> + '_ {
    let bytes = raw.as_bytes();
    let mut pos = 0;
    core::iter::from_fn(move || {
        if pos >= bytes.len() {
            return None;
        }
        let ws = is_ws(bytes[pos]);
        let start = pos;
        while pos < bytes.len() && is_ws(bytes[pos]) == ws {
            pos += 1;
        }
        debug_assert!(pos > start, "a chunk is never empty");
        debug_assert!(raw.is_char_boundary(pos), "separators are ASCII");
        Some(Chunk { start, end: pos, is_ws: ws })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn parts(raw: &str) -> Vec<(&str, bool)> {
        chunks(raw).map(|c| (&raw[c.start..c.end], c.is_ws)).collect()
    }

    #[test]
    fn splits_on_space_and_tab_runs_only() {
        assert_eq!(parts("a  b\tc "), alloc::vec![("a", false), ("  ", true), ("b", false), ("\t", true), ("c", false), (" ", true)]);
        assert_eq!(parts(""), alloc::vec![]);
        assert_eq!(parts("买菜 +家务"), alloc::vec![("买菜", false), (" ", true), ("+家务", false)]);
        assert_eq!(parts("a\u{a0}b"), alloc::vec![("a\u{a0}b", false)], "NBSP is not a separator");
    }
}
