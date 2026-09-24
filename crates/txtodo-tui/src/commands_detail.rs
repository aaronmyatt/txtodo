//! The detail panel's commands (task `tui-revamp/tui-detail`): open a line, close the panel, go up
//! a level or back to a breadcrumb, move between its parts, and mark the parent done. Pure like
//! `commands.rs`: what needs the daemon comes back as an [`Action`]. Leaving a level hands back its
//! unsaved notes as [`Action::SaveNotes`], so nothing typed is lost.

use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::app_detail::task_ref;
use crate::commands::{apply_of, today_local};
use crate::keymap::Command;
use crate::state::{AppState, EditDraft, LineState};
use crate::state_detail::{Parent, Part};
use crate::state_nav::Focus;

/// Runs a detail command; `None` for any other command.
pub fn run(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    Some(match command {
        Command::DetailOpen => open(state),
        Command::DetailClose => leave(state, 0),
        Command::DetailUp => {
            let keep = state.detail.levels.len().saturating_sub(1);
            leave(state, keep)
        }
        Command::DetailNextPart => focus_part(state, state.detail.part.next()),
        Command::DetailPrevPart => focus_part(state, state.detail.part.prev()),
        Command::DetailEditParent => focus_part(state, Part::Parent),
        Command::DetailEditNotes => focus_part(state, Part::Notes),
        Command::DetailStartSublist => focus_part(state, Part::Sub),
        Command::DetailCompleteParent => complete_parent(state),
        _ => return None,
    })
}

/// Enter on a list row: open it as a level (from the root list, or deeper from a sub-list, which
/// the input layer has swapped in). A blank line or the Add-a-line row has nothing to open.
fn open(state: &AppState) -> Option<Action> {
    let line = state.selected_line()?;
    if line.raw.trim().is_empty() {
        return None;
    }
    Some(Action::OpenDetail(parent_of(&state.path, line)))
}

/// A list line as a level's parent.
fn parent_of(path: &str, line: &LineState) -> Parent {
    Parent {
        path: path.to_owned(),
        line_number: line.line_number,
        task_id: line.task_ref_id().to_owned(),
        raw: line.raw.clone(),
        completed: line.completed,
    }
}

/// Keeps the first `keep` levels (0 closes the panel), handing back the shown level's notes when
/// they are unsaved.
pub fn leave(state: &mut AppState, keep: usize) -> Option<Action> {
    let save = state
        .detail
        .top()
        .filter(|l| l.notes.dirty() && state.detail.levels.len() > keep)
        .map(|l| Action::SaveNotes(task_ref(&l.parent), l.notes.text.clone()));
    state.detail.levels.truncate(keep);
    state.detail.part = Part::Sub;
    if keep == 0 {
        state.nav.focus = Focus::List;
    }
    state.rewatch = true;
    save
}

/// Gives `part` the keyboard. The parent field starts a fresh draft of the parent line.
fn focus_part(state: &mut AppState, part: Part) -> Option<Action> {
    let level = state.detail.top_mut()?;
    level.parent_draft = (part == Part::Parent).then(|| {
        let line = LineState::from_raw(level.parent.line_number, level.parent.raw.clone());
        EditDraft::for_line(&line, false)
    });
    state.detail.part = part;
    None
}

/// Whether every task line of the shown sub-list is done, and there is at least one.
pub fn all_sub_tasks_done(state: &AppState) -> bool {
    state.detail.top().is_some_and(|l| {
        let mut tasks = l
            .doc
            .lines
            .iter()
            .filter(|x| !x.raw.trim().is_empty())
            .peekable();
        tasks.peek().is_some() && tasks.all(|x| x.completed)
    })
}

/// Completes the parent once its sub-list is all done (desktop's "Mark parent done").
fn complete_parent(state: &mut AppState) -> Option<Action> {
    let parent = state.detail.top()?.parent.clone();
    if parent.completed || !all_sub_tasks_done(state) {
        return None;
    }
    let mutation = pb::Mutation {
        kind: Some(pb::mutation::Kind::Complete(pb::Complete {
            task: Some(task_ref(&parent)),
            today: today_local(),
        })),
    };
    let mut req = apply_of(state, mutation);
    req.path = parent.path;
    state.shell.pending_toast = Some("Marked the parent done".to_owned());
    Some(Action::Apply(req))
}

/// Enter in the parent field: saves the draft as an `Edit` of the parent line when it changed,
/// then hands the keyboard to the sub-list.
pub fn save_parent(state: &mut AppState) -> Option<Action> {
    let level = state.detail.top_mut()?;
    let draft = level.parent_draft.take()?;
    state.detail.part = Part::Sub;
    let level = state.detail.top()?;
    if draft.buffer == level.parent.raw {
        return None;
    }
    let mutation = pb::Mutation {
        kind: Some(pb::mutation::Kind::Edit(pb::Edit {
            task: Some(task_ref(&level.parent)),
            new_line: draft.buffer,
        })),
    };
    let mut req = apply_of(state, mutation);
    req.path.clone_from(&level.parent.path);
    Some(Action::Apply(req))
}

#[cfg(test)]
#[path = "commands_detail_tests.rs"]
mod tests;
