//! The Universal screen's state (task `tui-revamp/tui-universal`, c2 `c2/universal.js`): every
//! workspace's root-list tasks from `UniversalTasks`, and the filters over them. Which rows show,
//! and in which groups, is worked out here, purely, through core's `universal::group`, so the
//! screen, the keys and the mouse agree on one list.

use std::collections::{BTreeMap, BTreeSet};

use txtodo_core::universal::{GroupBy, RowFacts, days_between, group};

/// One task, the UI-local mirror of `pb::UniversalTask`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UTask {
    /// Its workspace's id.
    pub workspace_id: String,
    /// Its workspace's name (`default` for the default one).
    pub workspace: String,
    /// Its workspace's root list.
    pub root_list: String,
    /// Its 1-based line number there.
    pub line_number: u32,
    /// Its task id, `""` when the daemon holds none.
    pub task_id: String,
    /// The line.
    pub raw: String,
    /// Whether it is done.
    pub done: bool,
    /// Its priority letter.
    pub priority: Option<char>,
    /// Its raw `due:` value.
    pub due: Option<String>,
    /// Its projects and contexts, bare.
    pub projects: Vec<String>,
    /// Its contexts, bare.
    pub contexts: Vec<String>,
    /// Its `ref:` sub-list's done/total, when it has one.
    pub progress: Option<(u32, u32)>,
    /// Its `ref:` folder holds notes.
    pub has_notes: bool,
}

/// The groupings, in the selector's order.
pub const GROUPS: [(GroupBy, &str); 5] = [
    (GroupBy::Priority, "Priority"),
    (GroupBy::Due, "Due"),
    (GroupBy::Project, "Project"),
    (GroupBy::Context, "Context"),
    (GroupBy::Workspace, "Workspace"),
];

/// The screen's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniversalView {
    /// Every task, as the daemon last listed them.
    pub tasks: Vec<UTask>,
    /// The grouping.
    pub group: GroupBy,
    /// Show done tasks too.
    pub show_done: bool,
    /// Workspaces switched off (by id); at least one stays on.
    pub hidden: BTreeSet<String>,
    /// The one context shown, if chosen.
    pub context: Option<String>,
    /// The selected row, an index into [`UniversalView::rows`].
    pub cursor: usize,
}

impl Default for UniversalView {
    fn default() -> Self {
        UniversalView {
            tasks: Vec::new(),
            group: GroupBy::Priority,
            show_done: false,
            hidden: BTreeSet::new(),
            context: None,
            cursor: 0,
        }
    }
}

impl UniversalView {
    /// The workspaces, in the daemon's order: id, name, open count.
    pub fn workspaces(&self) -> Vec<(String, String, usize)> {
        let mut out: Vec<(String, String, usize)> = Vec::new();
        for t in &self.tasks {
            let open = usize::from(!t.done);
            match out.iter_mut().find(|(id, _, _)| *id == t.workspace_id) {
                Some(w) => w.2 += open,
                None => out.push((t.workspace_id.clone(), t.workspace.clone(), open)),
            }
        }
        out
    }

    /// The tasks in scope: shown workspaces, done ones only when asked for (or searched with
    /// `is:done`).
    fn in_scope(&self, query: &str) -> impl Iterator<Item = (usize, &UTask)> {
        let done_asked = self.show_done || query.to_lowercase().contains("is:done");
        self.tasks
            .iter()
            .enumerate()
            .filter(move |(_, t)| !self.hidden.contains(&t.workspace_id) && (!t.done || done_asked))
    }

    /// The contexts in scope with their counts, most used first.
    pub fn contexts(&self, query: &str) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, t) in self.in_scope(query) {
            for c in &t.contexts {
                *counts.entry(c).or_default() += 1;
            }
        }
        let mut out: Vec<(String, usize)> =
            counts.into_iter().map(|(c, n)| (c.to_owned(), n)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// The groups shown: each heading and the indexes (into `tasks`) of its rows, in order.
    pub fn groups(&self, query: &str, today: &str) -> Vec<(String, Vec<usize>)> {
        let shown: Vec<usize> = self
            .in_scope(query)
            .filter(|(_, t)| self.context.as_ref().is_none_or(|c| t.contexts.contains(c)))
            .filter(|(_, t)| txtodo_core::query::matches(&t.raw, query))
            .map(|(i, _)| i)
            .collect();
        let facts: Vec<RowFacts> = shown.iter().map(|&i| facts(&self.tasks[i])).collect();
        let names: Vec<String> = self.workspaces().into_iter().map(|(_, n, _)| n).collect();
        let order: Vec<&str> = names.iter().map(String::as_str).collect();
        group(&facts, self.group, today, &order)
            .into_iter()
            .map(|(name, members)| (name, members.into_iter().map(|m| shown[m]).collect()))
            .collect()
    }

    /// The rows shown, in display order: indexes into `tasks`.
    pub fn rows(&self, query: &str, today: &str) -> Vec<usize> {
        self.groups(query, today)
            .into_iter()
            .flat_map(|(_, members)| members)
            .collect()
    }

    /// Overdue, due this week, open and done, over the shown workspaces.
    pub fn stats(&self, today: &str) -> [usize; 4] {
        let mut out = [0; 4];
        for t in self
            .tasks
            .iter()
            .filter(|t| !self.hidden.contains(&t.workspace_id))
        {
            if t.done {
                out[3] += 1;
                continue;
            }
            out[2] += 1;
            match t.due.as_deref().and_then(|d| days_between(today, d)) {
                Some(d) if d < 0 => out[0] += 1,
                Some(d) if d <= 7 => out[1] += 1,
                _ => {}
            }
        }
        out
    }

    /// Switches workspace `id` on or off; the last one on stays on.
    pub fn toggle_workspace(&mut self, id: &str) {
        if !self.hidden.remove(id) {
            let on = self.workspaces().len() - self.hidden.len();
            if on > 1 {
                self.hidden.insert(id.to_owned());
            }
        }
        self.cursor = 0;
    }

    /// Shows only context `name`, or every context again when it is the one shown.
    pub fn toggle_context(&mut self, name: &str) {
        self.context = if self.context.as_deref() == Some(name) {
            None
        } else {
            Some(name.to_owned())
        };
        self.cursor = 0;
    }

    /// Every workspace on, every context, no done tasks: the empty state's Reset filters.
    pub fn reset(&mut self) {
        self.hidden.clear();
        self.context = None;
        self.show_done = false;
        self.cursor = 0;
    }
}

/// What core's grouping reads off a task.
fn facts(t: &UTask) -> RowFacts<'_> {
    RowFacts {
        done: t.done,
        priority: t.priority,
        due: t.due.as_deref(),
        project: t.projects.first().map(String::as_str),
        context: t.contexts.first().map(String::as_str),
        workspace: &t.workspace,
    }
}

#[cfg(test)]
#[path = "state_universal_tests.rs"]
mod tests;
