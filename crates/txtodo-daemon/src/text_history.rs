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
//!
//! One other way is kept instead: completing a line with a priority appends `pri:X` to its
//! description (`fields.rs`). That rewrite is recorded as [`Change::Append`] at its op's stamp, so a
//! text edit that arrives late is still slotted in by stamp, and the suffix lands at the end of
//! whatever text the replay builds. A splice would not: its offset is the end of the text its
//! device saw, which a late edit slotted in front of it moves (lab lan-converge seed 202).

use txtodo_model::{DeviceId, Hlc, TextEdit, Ulid};

use crate::textedit::TextEditError;

/// Most edits a task's description keeps before the oldest fold into its base.
pub(crate) const MAX_EDITS_PER_TASK: usize = 64;
/// Most edits a `notes.md` keeps before the oldest fold into its base.
pub(crate) const MAX_EDITS_PER_NOTES: usize = 256;

/// How a text applies a splice: descriptions and notes count differently (`textedit.rs`).
pub(crate) type ApplyEdits = fn(&str, &[TextEdit]) -> Result<String, TextEditError>;

/// One change a history keeps.
#[derive(Clone, Debug)]
pub(crate) enum Change {
    /// A splice at char offsets on the text its author saw: an `EditText` or `NotesEdit`.
    Splice(Vec<TextEdit>),
    /// Text added at the end, whatever text is there: a priority moved into `pri:`.
    Append(String),
}

impl Change {
    fn on(&self, text: &str, apply: ApplyEdits) -> Result<String, TextEditError> {
        match self {
            Change::Splice(edits) => apply(text, edits),
            Change::Append(suffix) => Ok(format!("{text}{suffix}")),
        }
    }
}

/// One text's base and the edits made on it since, oldest stamp first.
#[derive(Clone, Debug)]
pub(crate) struct TextHistory {
    base: String,
    /// When the base was set whole ([`TextHistory::reset`]): an edit older than that lost to it.
    /// Zero for a history begun from whatever text was there.
    since: Hlc,
    edits: Vec<(Hlc, Change)>,
}

impl TextHistory {
    /// A history starting at `base`, the text before its first edit.
    pub(crate) fn new(base: String) -> TextHistory {
        TextHistory {
            base,
            since: Hlc::zero(DeviceId::new(Ulid::from_u128(0))),
            edits: Vec::new(),
        }
    }

    /// The text set whole to `text` by an op stamped `at` (a task inserted again, task
    /// partition-converge): edits up to `at` are dropped, newer ones replayed on it. A set older
    /// than the one the base already took changes nothing. Returns the text to hold now.
    pub(crate) fn reset(&mut self, text: String, at: Hlc, apply: ApplyEdits) -> String {
        if at >= self.since {
            self.base = text;
            self.since = at;
            self.edits.retain(|(h, _)| *h > at);
        }
        self.replay(apply)
    }

    /// Records `edits` stamped `hlc` on a text that holds `current`, and returns the text to hold
    /// now. `Err` (nothing recorded) only when the edits are the newest and do not fit `current`,
    /// which is how a single device has always refused them.
    pub(crate) fn apply(
        &mut self,
        current: &str,
        hlc: Hlc,
        edits: &[TextEdit],
        how: (ApplyEdits, usize),
    ) -> Result<String, TextEditError> {
        self.record(current, hlc, Change::Splice(edits.to_vec()), how)
    }

    /// Records `suffix` appended to the end of the text by an op stamped `hlc` (module doc), and
    /// returns the text to hold now. Never refused: an append fits any text.
    pub(crate) fn append(
        &mut self,
        current: &str,
        hlc: Hlc,
        suffix: &str,
        how: (ApplyEdits, usize),
    ) -> String {
        let change = Change::Append(suffix.to_owned());
        self.record(current, hlc, change, how)
            .unwrap_or_else(|_| format!("{current}{suffix}"))
    }

    fn record(
        &mut self,
        current: &str,
        hlc: Hlc,
        change: Change,
        (apply, max): (ApplyEdits, usize),
    ) -> Result<String, TextEditError> {
        if hlc < self.since {
            // Made before the text was set whole: that set already decided the text.
            return Ok(current.to_owned());
        }
        let late = self.edits.last().is_some_and(|(newest, _)| *newest > hlc);
        let next = if late {
            let at = self.edits.partition_point(|(h, _)| *h <= hlc);
            self.edits.insert(at, (hlc, change));
            self.replay(apply)
        } else {
            let next = change.on(current, apply)?;
            self.edits.push((hlc, change));
            next
        };
        self.trim(apply, max);
        Ok(next)
    }

    /// The base with every kept edit applied in stamp order, one that no longer fits skipped.
    fn replay(&self, apply: ApplyEdits) -> String {
        self.edits
            .iter()
            .fold(self.base.clone(), |text, (_, change)| {
                change.on(&text, apply).unwrap_or(text)
            })
    }

    /// Folds the oldest edits into the base past `max`. An edit older than the new base that
    /// arrives later still applies, on the current text, as before this history existed.
    fn trim(&mut self, apply: ApplyEdits, max: usize) {
        while self.edits.len() > max {
            let (_, oldest) = self.edits.remove(0);
            if let Ok(text) = oldest.on(&self.base, apply) {
                self.base = text;
            }
        }
    }

    /// After a commit: edits a scratch replay recorded under `scratch` take the commit's stamp.
    pub(crate) fn settle(&mut self, scratch: Hlc, real: Hlc) {
        if self.since == scratch {
            self.since = real;
        }
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

    #[test]
    fn a_reset_keeps_only_newer_edits_and_older_ones_after_it_lose() {
        let mut history = TextHistory::new("line".to_owned());
        let apply = (apply_notes_edits as ApplyEdits, 64);
        let text = history.apply("line", at(30), &insert(4, " new"), apply);
        assert_eq!(text.as_deref(), Ok("line new"));
        assert_eq!(
            history.reset("LINE".to_owned(), at(20), apply.0),
            "LINE new"
        );
        let late = history.apply("LINE new", at(10), &insert(0, ">"), apply);
        assert_eq!(late.as_deref(), Ok("LINE new"));
        assert_eq!(history.reset("old".to_owned(), at(15), apply.0), "LINE new");
    }
}
