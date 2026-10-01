//! Text edits that converge whatever order they arrive in (ADR 0034, option T1 of
//! `tasks/partition-converge/notes.md`). An `EditText` or `NotesEdit` is a splice at char offsets
//! on the text its author saw, so two devices that apply two concurrent splices in arrival order
//! can end with two texts. A text's history keeps the text it started from and every edit since,
//! in stamp order: an edit that is the newest applies on the current text as before; one that
//! arrives late is slotted in by stamp and the text is rebuilt from the base, an edit that no
//! longer fits skipped. Every device ends with the base and the same edits in the same order.
//!
//! Kept by `DocState` per task (descriptions) and by `NotesState` (a whole `notes.md`); both clear
//! it when the text changes some other way, and both take it from the log replay at open.

use txtodo_model::{Hlc, TextEdit};

use crate::textedit::TextEditError;

/// Most edits a task's description keeps before the oldest fold into its base.
pub(crate) const MAX_EDITS_PER_TASK: usize = 64;
/// Most edits a `notes.md` keeps before the oldest fold into its base.
pub(crate) const MAX_EDITS_PER_NOTES: usize = 256;

/// How a text applies a splice: descriptions and notes count differently (`textedit.rs`).
pub(crate) type ApplyEdits = fn(&str, &[TextEdit]) -> Result<String, TextEditError>;

/// One text's base and the edits made on it since, oldest stamp first.
#[derive(Clone, Debug)]
pub(crate) struct TextHistory {
    base: String,
    edits: Vec<(Hlc, Vec<TextEdit>)>,
}

impl TextHistory {
    /// A history starting at `base`, the text before its first edit.
    pub(crate) fn new(base: String) -> TextHistory {
        TextHistory {
            base,
            edits: Vec::new(),
        }
    }

    /// Records `edits` stamped `hlc` on a text that holds `current`, and returns the text to hold
    /// now. `Err` (nothing recorded) only when the edits are the newest and do not fit `current`,
    /// which is how a single device has always refused them.
    pub(crate) fn apply(
        &mut self,
        current: &str,
        hlc: Hlc,
        edits: &[TextEdit],
        (apply, max): (ApplyEdits, usize),
    ) -> Result<String, TextEditError> {
        let late = self.edits.last().is_some_and(|(newest, _)| *newest > hlc);
        let next = if late {
            let at = self.edits.partition_point(|(h, _)| *h <= hlc);
            self.edits.insert(at, (hlc, edits.to_vec()));
            self.replay(apply)
        } else {
            let next = apply(current, edits)?;
            self.edits.push((hlc, edits.to_vec()));
            next
        };
        self.trim(apply, max);
        Ok(next)
    }

    /// The base with every kept edit applied in stamp order, one that no longer fits skipped.
    fn replay(&self, apply: ApplyEdits) -> String {
        self.edits
            .iter()
            .fold(self.base.clone(), |text, (_, edits)| {
                apply(&text, edits).unwrap_or(text)
            })
    }

    /// Folds the oldest edits into the base past `max`. An edit older than the new base that
    /// arrives later still applies, on the current text, as before this history existed.
    fn trim(&mut self, apply: ApplyEdits, max: usize) {
        while self.edits.len() > max {
            let (_, oldest) = self.edits.remove(0);
            if let Ok(text) = apply(&self.base, &oldest) {
                self.base = text;
            }
        }
    }

    /// After a commit: edits a scratch replay recorded under `scratch` take the commit's stamp.
    pub(crate) fn settle(&mut self, scratch: Hlc, real: Hlc) {
        for (h, _) in self.edits.iter_mut().filter(|(h, _)| *h == scratch) {
            *h = real;
        }
        self.edits.sort_by_key(|(h, _)| *h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textedit::apply_notes_edits;
    use txtodo_model::{DeviceId, Ulid};

    fn at(wall: u64) -> Hlc {
        Hlc {
            wall_ms: wall,
            counter: 0,
            device: DeviceId::new(Ulid::from_u128(1)),
        }
    }

    fn insert(at: usize, text: &str) -> Vec<TextEdit> {
        vec![TextEdit::Insert {
            at,
            text: text.to_owned(),
        }]
    }

    /// Applies `(stamp, edits)` in the given order, from "line" with an empty history.
    fn run(order: &[(u64, Vec<TextEdit>)], max: usize) -> String {
        let mut history = TextHistory::new("line".to_owned());
        let mut text = "line".to_owned();
        for (wall, edits) in order {
            text = history
                .apply(&text, at(*wall), edits, (apply_notes_edits, max))
                .unwrap_or(text);
        }
        text
    }

    #[test]
    fn every_arrival_order_ends_with_the_stamp_order() {
        let a = (10, insert(4, " a"));
        let b = (20, insert(4, " b"));
        let c = (30, insert(0, ">"));
        let want = run(&[a.clone(), b.clone(), c.clone()], 64);
        assert_eq!(want, ">line b a");
        for order in [
            [a.clone(), c.clone(), b.clone()],
            [b.clone(), a.clone(), c.clone()],
            [b.clone(), c.clone(), a.clone()],
            [c.clone(), a.clone(), b.clone()],
            [c.clone(), b.clone(), a.clone()],
        ] {
            assert_eq!(run(&order, 64), want, "{order:?}");
        }
    }

    #[test]
    fn a_late_edit_goes_under_the_newer_one_it_was_made_before() {
        // 20 deletes the first four chars; 10, made before it, appends. Either order: "!".
        let del = (20, vec![TextEdit::Delete { at: 0, len: 4 }]);
        let ins = (10, insert(4, "!"));
        assert_eq!(run(&[ins.clone(), del.clone()], 64), "!");
        assert_eq!(run(&[del, ins], 64), "!");
    }

    #[test]
    fn past_the_bound_the_oldest_edits_fold_into_the_base() {
        let mut history = TextHistory::new("x".to_owned());
        let mut text = "x".to_owned();
        for wall in 1..=5 {
            text = history
                .apply(&text, at(wall), &insert(0, "y"), (apply_notes_edits, 2))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(text, "yyyyyx");
        assert_eq!(history.edits.len(), 2);
        assert_eq!(history.base, "yyyx");
    }
}
