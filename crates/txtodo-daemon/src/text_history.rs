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
//! One other way is kept instead: a priority on a done line lives in its description as `pri:X`
//! (`fields.rs`). Completing the line adds the tag; that is recorded as [`Change::Pri`] at its
//! op's stamp, so a text edit that arrives late is still slotted in by stamp, and the tag is set on
//! whatever text the replay builds. A splice would not: its offsets are where the tag sat in the
//! text its device saw, which a late edit slotted in front of it moves (lab lan-converge seed 202).
//! A priority changed on the done line swaps the tag in place ([`TextHistory::swap_pri`]): the
//! priority is last writer wins (`fields.rs` drops an older one), so the winner's letter is the
//! one every recorded tag and the base hold, whatever order the completion and the swap arrive in.

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
    /// The first `pri:` tag set to this letter, else ` pri:X` added at the end: a priority moved
    /// into `pri:` by a completion ([`set_pri`]).
    Pri(char),
}

impl Change {
    fn on(&self, text: &str, apply: ApplyEdits) -> Result<String, TextEditError> {
        match self {
            Change::Splice(edits) => apply(text, edits),
            Change::Pri(letter) => Ok(set_pri(text, *letter, true)),
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

    /// Records `pri:` set to `letter` by a completion stamped `hlc` (module doc), and returns the
    /// text to hold now. Never refused: it fits any text.
    pub(crate) fn set_pri(
        &mut self,
        current: &str,
        hlc: Hlc,
        letter: char,
        how: (ApplyEdits, usize),
    ) -> String {
        self.record(current, hlc, Change::Pri(letter), how)
            .unwrap_or_else(|_| set_pri(current, letter, true))
    }

    /// A newer priority on the done line (module doc): every recorded tag and the base's take
    /// `letter`, and the text is rebuilt. Returns the text to hold now; `None` when it still holds
    /// another letter, a tag a text edit added (a `do` before 4b11aeb6 sent one), which this cannot
    /// retag: the caller drops the history then.
    pub(crate) fn swap_pri(&mut self, letter: char, apply: ApplyEdits) -> Option<String> {
        self.base = set_pri(&self.base, letter, false);
        for (_, change) in &mut self.edits {
            if let Change::Pri(held) = change {
                *held = letter;
            }
        }
        let text = self.replay(apply);
        (set_pri(&text, letter, false) == text).then_some(text)
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

/// `text` with its first `pri:` tag word set to `pri:<letter>`, else, if `add`, with
/// ` pri:<letter>` added at the end (no space on an empty text): what core's `Edit::set_tag` does
/// to a description. `fields::keep_in_history` checks the two agree before it records a
/// [`Change::Pri`].
pub(crate) fn set_pri(text: &str, letter: char, add: bool) -> String {
    let mut start = 0;
    for word in text.split(' ') {
        if is_pri(word) {
            let end = start + word.len();
            return format!("{}pri:{letter}{}", &text[..start], &text[end..]);
        }
        start += word.len() + 1;
    }
    if !add {
        text.to_owned()
    } else if text.is_empty() {
        format!("pri:{letter}")
    } else {
        format!("{text} pri:{letter}")
    }
}

/// Whether `text` holds a `pri:` tag word.
pub(crate) fn has_pri(text: &str) -> bool {
    text.split(' ').any(is_pri)
}

/// `pri:` with a value, as core reads a tag word.
fn is_pri(word: &str) -> bool {
    word.len() > "pri:".len() && word.starts_with("pri:")
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
    fn set_pri_swaps_the_first_tag_or_adds_one_at_the_end() {
        assert_eq!(set_pri("call mum", 'A', true), "call mum pri:A");
        assert_eq!(set_pri("call mum", 'A', false), "call mum");
        assert_eq!(set_pri("", 'A', true), "pri:A");
        assert_eq!(
            set_pri("call pri:B mum pri:C", 'A', false),
            "call pri:A mum pri:C"
        );
        assert_eq!(set_pri("pri: is empty", 'A', true), "pri: is empty pri:A");
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
