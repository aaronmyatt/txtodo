//! The one piece of the old todo.txt Loro document `NotesDoc` still needs: replaying a dual-indexed
//! `TextEdit` stream onto a `LoroText`. The task-list document, its op translation, review flags
//! and LWW registers went with the todo.txt mirror (ADR 0038).
//! Loro text API: <https://docs.rs/loro/latest/loro/struct.LoroText.html>

use loro::{LoroResult, LoroText};

/// Replays a `diff_text`-convention edit stream onto a Loro text.
///
/// The stream is dual-indexed (`txtodo_model::TextEdit`): a `Delete`'s `at` counts the *source*,
/// an `Insert`'s `at` counts the *target*, both in order. Loro holds one evolving string, so each
/// position is translated through two cursors — an insert before a later delete shifts that delete.
/// A naive single-cursor replay lands on the wrong characters whenever both kinds appear.
/// Positions are Unicode code points, matching `LoroText::len_unicode`:
/// <https://docs.rs/loro/latest/loro/struct.LoroText.html#method.insert>
pub(crate) fn replay_edits(text: &LoroText, edits: &[txtodo_core::TextEdit]) -> LoroResult<()> {
    let mut cursor = 0usize; // next unconsumed char of the source text
    let mut emitted = 0usize; // chars of the target already produced or kept
    for edit in edits {
        match edit {
            txtodo_core::TextEdit::Delete { at, len } => {
                debug_assert!(
                    *at >= cursor,
                    "a delete never moves backwards in the source"
                );
                let pos = emitted + (at - cursor);
                text.delete(pos, *len)?;
                cursor = at + len;
                emitted = pos;
            }
            txtodo_core::TextEdit::Insert { at, text: s } => {
                debug_assert!(
                    *at >= emitted,
                    "an insert never moves backwards in the target"
                );
                let keep = at - emitted;
                let pos = emitted + keep;
                text.insert(pos, s)?;
                cursor += keep;
                emitted = pos + s.chars().count();
            }
        }
    }
    Ok(())
}
