//! Pure logic behind the `diff_text` wasm export (see [`crate::wasm`]). Kept free of
//! `wasm_bindgen`/`JsValue` so it can be unit-tested with plain `cargo test` on the host target;
//! the wasm module is a thin `JsValue`-shaping wrapper around [`diff_segments`].
//!
//! `txtodo_core::diff_text` returns a sparse edit list (only the `Insert`/`Delete` runs, positions
//! in `a`'s char space — see `crates/txtodo-core/src/diff.rs`). The conflict-review `DiffView`
//! (`tasks/desktop-conflict-review/notes.md`) wants full coverage of both strings instead, so this
//! module fills the gaps between edits with `Equal` runs.

use txtodo_core::{TextEdit, diff_text};

/// One rendered run of a text diff. Matches the JS discriminated union `{ op, text }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffOp {
    /// Present, unchanged, on both sides.
    Equal,
    /// Present in `b` only.
    Insert,
    /// Present in `a` only.
    Delete,
}

/// One segment of a rendered diff: an [`DiffOp`] and the text it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffSegment {
    /// What kind of run this is.
    pub op: DiffOp,
    /// The run's text, taken from whichever side it belongs to.
    pub text: String,
}

/// Turns `a` and `b` into full-coverage `Equal`/`Insert`/`Delete` runs, in left-to-right order,
/// suitable for a `DiffView` to render directly (concatenating every run's text reconstructs `b`).
pub fn diff_segments(a: &str, b: &str) -> Vec<DiffSegment> {
    let chars: Vec<char> = a.chars().collect();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for edit in diff_text(a, b) {
        match edit {
            TextEdit::Insert { at, text } => {
                push_equal(&mut out, &chars, cursor, at);
                out.push(DiffSegment {
                    op: DiffOp::Insert,
                    text,
                });
                cursor = at;
            }
            TextEdit::Delete { at, len } => {
                push_equal(&mut out, &chars, cursor, at);
                out.push(DiffSegment {
                    op: DiffOp::Delete,
                    text: chars[at..at + len].iter().collect(),
                });
                cursor = at + len;
            }
        }
    }
    push_equal(&mut out, &chars, cursor, chars.len());
    out
}

/// Appends an `Equal` run over `chars[from..to]` when non-empty.
fn push_equal(out: &mut Vec<DiffSegment>, chars: &[char], from: usize, to: usize) {
    if from < to {
        out.push(DiffSegment {
            op: DiffOp::Equal,
            text: chars[from..to].iter().collect(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Concatenating every segment's text reconstructs `b`.
    fn rendered(segments: &[DiffSegment]) -> String {
        segments.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn identical_strings_are_one_equal_run() {
        let segs = diff_segments("abc", "abc");
        assert_eq!(
            segs,
            vec![DiffSegment {
                op: DiffOp::Equal,
                text: "abc".into()
            }]
        );
    }

    #[test]
    fn insert_is_wrapped_in_equal_runs() {
        let segs = diff_segments("abc", "abXYc");
        assert_eq!(
            segs,
            vec![
                DiffSegment {
                    op: DiffOp::Equal,
                    text: "ab".into()
                },
                DiffSegment {
                    op: DiffOp::Insert,
                    text: "XY".into()
                },
                DiffSegment {
                    op: DiffOp::Equal,
                    text: "c".into()
                },
            ]
        );
        assert_eq!(rendered(&segs), "abXYc");
    }

    #[test]
    fn delete_is_wrapped_in_equal_runs() {
        let segs = diff_segments("abXYc", "abc");
        assert_eq!(
            segs,
            vec![
                DiffSegment {
                    op: DiffOp::Equal,
                    text: "ab".into()
                },
                DiffSegment {
                    op: DiffOp::Delete,
                    text: "XY".into()
                },
                DiffSegment {
                    op: DiffOp::Equal,
                    text: "c".into()
                },
            ]
        );
    }

    #[test]
    fn char_not_byte_positions_for_multibyte_text() {
        let segs = diff_segments("买菜", "买好菜");
        assert_eq!(
            segs,
            vec![
                DiffSegment {
                    op: DiffOp::Equal,
                    text: "买".into()
                },
                DiffSegment {
                    op: DiffOp::Insert,
                    text: "好".into()
                },
                DiffSegment {
                    op: DiffOp::Equal,
                    text: "菜".into()
                },
            ]
        );
    }

    #[test]
    fn empty_original_is_one_insert_run() {
        assert_eq!(
            diff_segments("", "ab"),
            vec![DiffSegment {
                op: DiffOp::Insert,
                text: "ab".into()
            }]
        );
    }
}
