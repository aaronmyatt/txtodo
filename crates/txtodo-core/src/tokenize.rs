//! `tokenize`: classify every byte of a line for highlighters. Never fails; unknown words are `Text`.
//! The expected output for real lines is `corpus/*.tokens.json`; that oracle is the contract.

use crate::scanner::{chunks, Chunk};
use crate::urls::{is_url, DEFAULT_SCHEMES};
use crate::{Date, Priority, Span, TokenKind, Ulid};
use alloc::vec::Vec;

/// Tokenizes with [`DEFAULT_SCHEMES`]. Spans are contiguous and cover `[0, raw.len())`; `""` gives `[]`.
pub fn tokenize(raw: &str) -> Vec<Span> {
    tokenize_with_schemes(raw, DEFAULT_SCHEMES)
}

/// Tokenizes with a custom URL scheme list.
pub fn tokenize_with_schemes<'a>(raw: &'a str, schemes: &'a [&'a str]) -> Vec<Span> {
    let mut t = Tokenizer { raw, schemes, spans: Vec::new(), prefix: Prefix::default() };
    for (index, chunk) in chunks(raw).enumerate() {
        t.push_chunk(chunk, index);
    }
    debug_assert!(t.spans.iter().all(|s| s.start < s.end), "no empty spans");
    debug_assert!(t.spans.last().is_none_or(|s| s.end == raw.len()), "spans cover the line");
    t.spans
}

/// Where we are in the structured prefix (`x`, dates, priority) before the description starts.
#[derive(Default)]
struct Prefix {
    done: bool,
    seen_x: bool,
    dates: u8,
    seen_priority: bool,
}

/// One tokenization in progress.
struct Tokenizer<'a> {
    raw: &'a str,
    schemes: &'a [&'a str],
    spans: Vec<Span>,
    prefix: Prefix,
}

impl Tokenizer<'_> {
    /// Appends the span(s) for one chunk.
    fn push_chunk(&mut self, chunk: Chunk, index: usize) {
        let Chunk { start, end, is_ws } = chunk;
        if is_ws {
            self.spans.push(Span { kind: TokenKind::Whitespace, start, end });
            return;
        }
        let word = &self.raw[start..end];
        if !self.prefix.done {
            match classify_prefix(word, index, &mut self.prefix) {
                Some(kind) => return self.spans.push(Span { kind, start, end }),
                None => self.prefix.done = true,
            }
        }
        push_word(&mut self.spans, word, start, self.schemes);
    }
}

/// Position-based prefix classification; `None` ends the prefix. Lenient orders (`x (A) date`,
/// `x date (A)`) still classify; the parser is what records them as quirks.
fn classify_prefix(word: &str, index: usize, p: &mut Prefix) -> Option<TokenKind> {
    if index == 0 && word == "x" {
        p.seen_x = true;
        return Some(TokenKind::CompletionMarker);
    }
    if Date::parse(word).is_some() {
        return classify_date(p);
    }
    if !p.seen_priority && is_priority_word(word) {
        p.seen_priority = true;
        return Some(TokenKind::Priority);
    }
    None
}

/// After `x`: first date is completion, second is creation. Otherwise the only date is creation.
fn classify_date(p: &mut Prefix) -> Option<TokenKind> {
    let max_dates = if p.seen_x { 2 } else { 1 };
    debug_assert!(p.dates <= max_dates, "never counts past the maximum");
    if p.dates >= max_dates {
        return None;
    }
    p.dates += 1;
    let is_completion = p.seen_x && p.dates == 1;
    Some(if is_completion { TokenKind::CompletionDate } else { TokenKind::CreationDate })
}

/// `(A)`–`(Z)` exactly.
pub(crate) fn is_priority_word(word: &str) -> bool {
    let b = word.as_bytes();
    b.len() == 3 && b[0] == b'(' && b[2] == b')' && Priority::new(char::from(b[1])).is_some()
}

/// What a description word is. One classifier for the tokenizer, the parser and the task views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordKind {
    /// Recognised scheme and a colon.
    Url,
    /// `id:` plus a valid ULID.
    IdTag,
    /// `+x`.
    Project,
    /// `@x`.
    Context,
    /// `key:value`; the byte index of the colon.
    Tag(usize),
    /// Anything else.
    Text,
}

/// Classifies one description word in the order design §2.4 requires: URL first, then id, sigils, tags.
pub(crate) fn classify_word(word: &str, schemes: &[&str]) -> WordKind {
    debug_assert!(!word.is_empty(), "scanner never yields empty words");
    if is_url(word, schemes) {
        return WordKind::Url;
    }
    if word.strip_prefix("id:").and_then(Ulid::parse).is_some() {
        return WordKind::IdTag;
    }
    if word.len() > 1 && word.starts_with('+') {
        return WordKind::Project;
    }
    if word.len() > 1 && word.starts_with('@') {
        return WordKind::Context;
    }
    tag_split(word).map_or(WordKind::Text, WordKind::Tag)
}

/// Pushes the span(s) for one description word.
fn push_word(spans: &mut Vec<Span>, word: &str, start: usize, schemes: &[&str]) {
    let end = start + word.len();
    let kind = match classify_word(word, schemes) {
        WordKind::Url => TokenKind::Url,
        WordKind::IdTag => TokenKind::IdTag,
        WordKind::Project => TokenKind::Project,
        WordKind::Context => TokenKind::Context,
        WordKind::Text => TokenKind::Text,
        WordKind::Tag(colon) => {
            spans.push(Span { kind: TokenKind::TagKey, start, end: start + colon + 1 });
            spans.push(Span { kind: TokenKind::TagValue, start: start + colon + 1, end });
            return;
        }
    };
    spans.push(Span { kind, start, end });
}

/// Byte index of the colon in a `key:value` word with non-empty key and value; `None` otherwise.
pub(crate) fn tag_split(word: &str) -> Option<usize> {
    let colon = word.find(':')?;
    debug_assert!(colon < word.len(), "find returns an in-range index");
    (colon > 0 && colon + 1 < word.len()).then_some(colon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use TokenKind::*;

    fn kinds(raw: &str) -> Vec<TokenKind> {
        tokenize(raw).into_iter().map(|s| s.kind).collect()
    }

    #[test]
    fn design_2_4_rows() {
        assert_eq!(kinds("mail bob@example.com"), [Text, Whitespace, Text]);
        assert_eq!(kinds("see https://example.com/x"), [Text, Whitespace, Url]);
        assert_eq!(kinds("learn C++ +cpp"), [Text, Whitespace, Text, Whitespace, Project]);
        assert_eq!(kinds("买菜 +家务 @手机"), [Text, Whitespace, Project, Whitespace, Context]);
        assert_eq!(kinds("X 2026-09-11 not done"), [Text, Whitespace, Text, Whitespace, Text, Whitespace, Text]);
        assert_eq!(kinds("x 2026-09-11 (A) task"), [CompletionMarker, Whitespace, CompletionDate, Whitespace, Priority, Whitespace, Text]);
        assert_eq!(kinds("note: buy milk"), [Text, Whitespace, Text, Whitespace, Text]);
        assert_eq!(kinds("(a) task"), [Text, Whitespace, Text]);
    }

    #[test]
    fn prefix_and_tags() {
        assert_eq!(kinds("(A) 2026-09-11 t due:2026-09-15"), [Priority, Whitespace, CreationDate, Whitespace, Text, Whitespace, TagKey, TagValue]);
        assert_eq!(kinds("x 2026-09-11 2026-09-01 t pri:A"), [CompletionMarker, Whitespace, CompletionDate, Whitespace, CreationDate, Whitespace, Text, Whitespace, TagKey, TagValue]);
        assert_eq!(kinds("2026-09-11 2026-09-01 t"), [CreationDate, Whitespace, Text, Whitespace, Text], "second date is text");
        assert_eq!(kinds("t id:01J9K3H5Z7Q8X2M4N6P8R0T2V4"), [Text, Whitespace, IdTag]);
        assert_eq!(kinds("t id:short"), [Text, Whitespace, TagKey, TagValue], "bad ulid is an ordinary tag");
        assert_eq!(kinds("a:b:c"), [TagKey, TagValue]);
        assert_eq!(tokenize("a:b:c")[0].end, 2, "key span includes the colon");
    }

    #[test]
    fn spans_cover_every_byte_of_every_corpus_line() {
        let corpus = [
            include_str!("../../../corpus/edge-cases.txt"),
            include_str!("../../../corpus/lenient.txt"),
            include_str!("../../../corpus/tags.txt"),
            include_str!("../../../corpus/refs.txt"),
            include_str!("../../../corpus/structure.txt"),
        ];
        for line in corpus.iter().flat_map(|f| f.lines()) {
            let spans = tokenize(line);
            let mut pos = 0;
            for s in &spans {
                assert_eq!(s.start, pos, "{line:?}: gap before {s:?}");
                assert!(line.is_char_boundary(s.end), "{line:?}: span ends mid-char");
                pos = s.end;
            }
            assert_eq!(pos, line.len(), "{line:?}: spans stop early");
        }
        assert!(tokenize("").is_empty());
    }
}
