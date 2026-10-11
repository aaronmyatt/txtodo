//! Change notices (task `change-notifications`): a long-running client (desktop, TUI) tells the
//! human when someone else edits, completes or deletes a task. The daemon's `Watch` stream sends
//! each change's ops with the client that made them (`OpSummary.source`); this module drops the
//! client's own ops, folds a burst into one notice, and words it. Timing is the caller's: it
//! passes `now` in ms and asks [`Debouncer::due`] when [`Debouncer::next_due`] says.
//!
//! Sources, as the daemon stamps them: a client's own name (`desktop`, `tui`, `cli`, `mcp`),
//! `sync` for an op from another device, `external` for a save in an editor.
//! Kinds and summaries: `crates/txtodo-store/src/ops.rs::kind_tag` and
//! `crates/txtodo-daemon/src/convert.rs::summary_of` (`Completed = Bool(true)` and the like).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Quiet this long after the last op and the notice goes out.
pub const QUIET_MS: u64 = 3_000;
/// A burst that never goes quiet still gets a notice this long after its first op.
pub const MAX_WAIT_MS: u64 = 15_000;

/// What happened to a task. Ordered: one task's strongest verb wins in a burst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    /// Its text, priority or tags changed, or it was reopened.
    Edited,
    /// Marked done.
    Completed,
    /// Deleted.
    Deleted,
}

impl Verb {
    fn word(self) -> &'static str {
        match self {
            Verb::Edited => "edited",
            Verb::Completed => "completed",
            Verb::Deleted => "deleted",
        }
    }
}

/// The verb an op stands for, or `None` for one not worth a notice: an add, a move, a blank line,
/// a notes edit, the completion date that rides along with a completion.
pub fn verb_of(kind: &str, summary: &str) -> Option<Verb> {
    match kind {
        "edit_text" | "remove_tag" => Some(Verb::Edited),
        "set_field" if summary.starts_with("Completed = Bool(true)") => Some(Verb::Completed),
        "set_field" if summary.starts_with("Deleted = Bool(true)") => Some(Verb::Deleted),
        "set_field" if summary.starts_with("CompletionDate") || summary.starts_with("Quirks") => {
            None
        }
        "set_field" => Some(Verb::Edited),
        _ => None,
    }
}

/// One op from a `Watch` change, the fields this module reads.
#[derive(Debug, Clone, Copy)]
pub struct OpFacts<'a> {
    /// The change's document, workspace-relative.
    pub path: &'a str,
    /// `OpSummary.source`.
    pub source: &'a str,
    /// `OpSummary.kind`.
    pub kind: &'a str,
    /// `OpSummary.task_id`; empty for a blank-line op.
    pub task_id: &'a str,
    /// `OpSummary.summary`.
    pub summary: &'a str,
}

/// What to show: a title and a one-line body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// "A task was completed by an agent", "3 tasks changed on another device".
    pub title: String,
    /// The counts and where: "1 completed, 2 edited, in todo.txt".
    pub body: String,
}

/// Folds other clients' ops into one notice per burst.
#[derive(Debug, Clone)]
pub struct Debouncer {
    own_source: String,
    first_at: Option<u64>,
    last_at: u64,
    /// (path, task id) → the strongest verb seen.
    tasks: BTreeMap<(String, String), Verb>,
    /// Every source seen this burst, for the "by whom" words.
    sources: Vec<String>,
}

impl Debouncer {
    /// A debouncer for the client named `own_source` (`"desktop"`, `"tui"`): its ops never count.
    pub fn new(own_source: &str) -> Self {
        Self {
            own_source: own_source.to_string(),
            first_at: None,
            last_at: 0,
            tasks: BTreeMap::new(),
            sources: Vec::new(),
        }
    }

    /// Takes one op at `now_ms`; true when it counts toward a notice.
    pub fn push(&mut self, now_ms: u64, op: OpFacts<'_>) -> bool {
        if op.source == self.own_source || op.task_id.is_empty() {
            return false;
        }
        let Some(verb) = verb_of(op.kind, op.summary) else {
            return false;
        };
        let slot = self
            .tasks
            .entry((op.path.to_string(), op.task_id.to_string()))
            .or_insert(verb);
        *slot = (*slot).max(verb);
        if !self.sources.iter().any(|s| s == op.source) {
            self.sources.push(op.source.to_string());
        }
        self.first_at.get_or_insert(now_ms);
        self.last_at = now_ms;
        true
    }

    /// When the pending notice is due, if one is pending.
    pub fn next_due(&self) -> Option<u64> {
        let first = self.first_at?;
        Some((self.last_at + QUIET_MS).min(first + MAX_WAIT_MS))
    }

    /// The notice, once it is due at `now_ms`; the burst starts over.
    pub fn due(&mut self, now_ms: u64) -> Option<Notice> {
        if now_ms < self.next_due()? {
            return None;
        }
        let notice = word(&self.tasks, &self.sources);
        self.first_at = None;
        self.tasks.clear();
        self.sources.clear();
        Some(notice)
    }
}

/// "by an agent", "on another device"..., or "" when several kinds of source mixed.
fn by_whom(sources: &[String]) -> &'static str {
    match sources {
        [one] => match one.as_str() {
            "sync" => " on another device",
            "mcp" => " by an agent",
            "cli" => " from the CLI",
            "tui" => " from the TUI",
            "desktop" => " from the desktop app",
            "external" => " in an editor",
            _ => "",
        },
        _ => "",
    }
}

fn word(tasks: &BTreeMap<(String, String), Verb>, sources: &[String]) -> Notice {
    let who = by_whom(sources);
    let mut paths: Vec<&str> = tasks.keys().map(|(p, _)| p.as_str()).collect();
    paths.dedup();
    let place = match paths.as_slice() {
        [one] => format!("in {one}"),
        [a, b] => format!("in {a} and {b}"),
        [a, rest @ ..] => format!("in {a} and {} more lists", rest.len()),
        [] => String::new(),
    };
    if tasks.len() == 1 {
        let verb = tasks.values().next().map_or("edited", |v| v.word());
        return Notice {
            title: format!("A task was {verb}{who}"),
            body: place,
        };
    }
    let counts: Vec<String> = [Verb::Completed, Verb::Deleted, Verb::Edited]
        .into_iter()
        .filter_map(|v| {
            let n = tasks.values().filter(|&&t| t == v).count();
            (n > 0).then(|| format!("{n} {}", v.word()))
        })
        .collect();
    Notice {
        title: format!("{} tasks changed{who}", tasks.len()),
        body: format!("{}, {place}", counts.join(", ")),
    }
}

#[cfg(test)]
#[path = "notice_tests.rs"]
mod tests;
