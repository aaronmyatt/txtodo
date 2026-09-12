//! Applies char-level `TextEdit`s to a description. The edits are `txtodo_core::diff_text`'s stream:
//! a `Delete` is addressed in the source text, an `Insert` in the target text, both in order — so the
//! applier walks a source cursor and a target cursor together. EditText ops carry exactly this.

use std::fmt;
use txtodo_model::TextEdit;

/// Most edits one EditText may carry; a description is one line, so this is generous.
pub const MAX_TEXT_EDITS: usize = 10_000;

/// An edit that does not fit the text it is applied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditError {
    /// Index of the offending edit.
    pub index: usize,
    /// Char position it referred to.
    pub at: usize,
    /// Char length of the source text.
    pub len: usize,
}

impl fmt::Display for TextEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "text edit {} at char {} does not fit a {}-char text",
            self.index, self.at, self.len
        )
    }
}

impl std::error::Error for TextEditError {}

/// Applies `edits` to `source`, producing the target text. Shared by descriptions (one line, via
/// [`apply_text_edits`]) and `notes.md` prose (may contain newlines, via [`apply_notes_edits`]) —
/// the two wrappers differ only in which invariant they then assert on the result.
fn apply_text_edits_core(source: &str, edits: &[TextEdit]) -> Result<String, TextEditError> {
    debug_assert!(
        edits.len() <= MAX_TEXT_EDITS,
        "EditText carries at most {MAX_TEXT_EDITS} edits"
    );
    let src: Vec<char> = source.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(src.len());
    let mut cursor = 0usize; // next unconsumed char of `src`
    for (index, edit) in edits.iter().enumerate().take(MAX_TEXT_EDITS) {
        let fail = |at: usize| TextEditError {
            index,
            at,
            len: src.len(),
        };
        match edit {
            TextEdit::Delete { at, len } => {
                let end = at
                    .checked_add(*len)
                    .filter(|e| *e <= src.len() && *at >= cursor)
                    .ok_or(fail(*at))?;
                out.extend_from_slice(&src[cursor..*at]);
                cursor = end;
            }
            TextEdit::Insert { at, text } => {
                let keep = at.checked_sub(out.len()).ok_or(fail(*at))?;
                let end = cursor
                    .checked_add(keep)
                    .filter(|e| *e <= src.len())
                    .ok_or(fail(*at))?;
                out.extend_from_slice(&src[cursor..end]);
                cursor = end;
                out.extend(text.chars());
            }
        }
    }
    out.extend_from_slice(&src[cursor..]);
    Ok(out.into_iter().collect())
}

/// Applies `edits` to a description `source`, producing the target text.
pub fn apply_text_edits(source: &str, edits: &[TextEdit]) -> Result<String, TextEditError> {
    let out = apply_text_edits_core(source, edits)?;
    debug_assert!(
        !out.contains('\n'),
        "descriptions never contain line breaks"
    );
    Ok(out)
}

/// Applies `edits` to `notes.md` prose, which may contain newlines (plan M5).
pub fn apply_notes_edits(source: &str, edits: &[TextEdit]) -> Result<String, TextEditError> {
    apply_text_edits_core(source, edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use txtodo_core::diff_text;

    #[test]
    fn deletes_address_the_source_and_inserts_address_the_target() {
        // "abcdefg" → "abXYZdeg": delete "c" (source 2), insert "XYZ" at target 2, delete "f" (source 5)
        let edits = [
            TextEdit::Delete { at: 2, len: 1 },
            TextEdit::Insert {
                at: 2,
                text: "XYZ".into(),
            },
            TextEdit::Delete { at: 5, len: 1 },
        ];
        assert_eq!(apply_text_edits("abcdefg", &edits).unwrap(), "abXYZdeg");
        let err = apply_text_edits("ab", &[TextEdit::Delete { at: 1, len: 5 }]).unwrap_err();
        assert_eq!(
            err,
            TextEditError {
                index: 0,
                at: 1,
                len: 2
            }
        );
        assert_eq!(
            apply_text_edits("héllo", &[TextEdit::Delete { at: 1, len: 1 }]).unwrap(),
            "hllo"
        );
    }

    proptest! {
        #[test]
        fn applying_diff_text_reproduces_the_target(a in "[a-c ]{0,12}", b in "[a-c é]{0,12}") {
            let edits: Vec<TextEdit> = diff_text(&a, &b).into_iter().map(TextEdit::from).collect();
            prop_assert_eq!(apply_text_edits(&a, &edits).unwrap(), b);
        }
    }

    #[test]
    fn notes_edits_may_contain_newlines() {
        let a = "line one\nline two\n";
        let b = "line one\nline TWO\nline three\n";
        let edits: Vec<TextEdit> = diff_text(a, b).into_iter().map(TextEdit::from).collect();
        assert_eq!(apply_notes_edits(a, &edits).unwrap(), b);
    }
}
