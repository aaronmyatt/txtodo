//! What each [`Command`] does (task `tui-revamp/tui-foundation`): the one path the keyboard, the
//! `:` palette and (later) mouse hits all go through. State-only commands change `AppState` here;
//! anything the daemon must do comes back as an [`Action`] for `app::perform`. Pure and
//! daemon-free, so it is tested without a terminal.

use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::keymap::Command;
use crate::state::{AppState, Resolution};
use crate::ui::edit::{self, OpenKey};
use crate::ui::{conflicts, offers};

/// What the status line says when a command would change a line under review.
pub const READ_ONLY: &str = "that line needs review first: r";

/// Runs one command against `state`: the daemon call it needs, if any.
pub fn run(state: &mut AppState, command: Command) -> Option<Action> {
    if changes_the_line(command) && under_review(state, state.cursor) {
        state.last_error = Some(READ_ONLY.to_owned());
        return None;
    }
    match command {
        Command::ListDown => state.move_down(),
        Command::ListUp => state.move_up(),
        Command::ListFirst => state.move_first(),
        Command::ListLast => state.move_last(),
        Command::ListEditStart => edit::start(state, OpenKey::Insert),
        Command::ListEditEnd => edit::start(state, OpenKey::AppendEnd),
        Command::ListDelete => {
            let m = delete_selected(state);
            return toasting(state, m, "Deleted the line");
        }
        Command::ListToggleComplete => {
            let done = state.selected_line().is_some_and(|l| l.completed);
            let m = toggle_complete(state);
            let message = if done {
                "Reopened the line"
            } else {
                "Completed the line"
            };
            return toasting(state, m, message);
        }
        Command::ListMoveDown => {
            let m = move_selected_down(state);
            return apply(state, m);
        }
        Command::ListMoveUp => {
            let m = move_selected_up(state);
            return apply(state, m);
        }
        Command::EditCommit => {
            let m = edit::commit(state);
            return apply(state, m);
        }
        Command::EditCancel => edit::cancel(state),
        other => return run_panes(state, other),
    }
    None
}

/// Whether `command` edits, moves, completes or deletes the selected line.
fn changes_the_line(command: Command) -> bool {
    matches!(
        command,
        Command::ListEditStart
            | Command::ListEditEnd
            | Command::ListDelete
            | Command::ListToggleComplete
            | Command::ListMoveDown
            | Command::ListMoveUp
    )
}

/// Whether the line at `idx` carries a `needs_review` flag: it is read-only until the flag is
/// resolved, like desktop's buffer (per line here, since the TUI edits line by line).
pub(crate) fn under_review(state: &AppState, idx: usize) -> bool {
    let Some(line) = state.lines.get(idx) else {
        return false;
    };
    let id = line.task_ref_id();
    state
        .needs_review
        .iter()
        .any(|f| (!id.is_empty() && f.task_id == id) || f.line_number == line.line_number)
}

/// The conflict and offer sheets, the sync pane, the palette and quitting.
fn run_panes(state: &mut AppState, command: Command) -> Option<Action> {
    match command {
        Command::ConflictsOpen | Command::ConflictsClose => state.toggle_conflicts(),
        Command::ConflictsDown => conflicts::move_down(state),
        Command::ConflictsUp => conflicts::move_up(state),
        Command::ConflictsKeepMine => return resolve(state, Resolution::Mine),
        Command::ConflictsKeepTheirs => return resolve(state, Resolution::Theirs),
        Command::ConflictsKeepMerged => return resolve(state, Resolution::Merged),
        Command::OffersOpen | Command::OffersClose => state.offers.toggle(),
        Command::OffersDown => state.offers.move_down(),
        Command::OffersUp => state.offers.move_up(),
        Command::OffersAccept => return offers::accept_request(state).map(Action::AcceptOffer),
        Command::OffersDecline => return offers::decline_request(state).map(Action::DeclineOffer),
        Command::SyncOpen => state.toggle_sync_visible(),
        Command::PaletteOpen => state.start_command(),
        Command::AppQuit => return Some(Action::Quit),
        other => {
            let run = crate::commands_nav::run(state, other)
                .or_else(|| crate::search::run(state, other))
                .or_else(|| crate::commands_detail::run(state, other));
            return run.flatten();
        }
    }
    None
}

/// [`apply`], and the toast (with Undo) to show once the daemon has it.
fn toasting(state: &mut AppState, mutation: Option<pb::Mutation>, message: &str) -> Option<Action> {
    let action = apply(state, mutation)?;
    state.shell.pending_toast = Some(message.to_owned());
    Some(action)
}

fn apply(state: &AppState, mutation: Option<pb::Mutation>) -> Option<Action> {
    mutation.map(|m| Action::Apply(apply_of(state, m)))
}

fn resolve(state: &AppState, pick: Resolution) -> Option<Action> {
    conflicts::resolve_request(state, pick).map(Action::Resolve)
}

/// `Space`: builds the `Complete` mutation for the selected line, if any (the Add-a-line row has
/// nothing to complete).
fn toggle_complete(state: &AppState) -> Option<pb::Mutation> {
    let line = state.selected_line()?;
    Some(pb::Mutation {
        kind: Some(pb::mutation::Kind::Complete(pb::Complete {
            task: Some(pb::TaskRef {
                line_number: line.line_number,
                task_id: line.task_ref_id().to_owned(),
            }),
            today: today_local(),
        })),
    })
}

/// `dd`: builds the `Delete` mutation for the selected line; `leave_blank = true` matches
/// todo.sh's own default (proto's own doc on `Delete`).
fn delete_selected(state: &AppState) -> Option<pb::Mutation> {
    let line = state.selected_line()?;
    Some(pb::Mutation {
        kind: Some(pb::mutation::Kind::Delete(pb::Delete {
            task: Some(pb::TaskRef {
                line_number: line.line_number,
                task_id: line.task_ref_id().to_owned(),
            }),
            leave_blank: true,
        })),
    })
}

/// The `TaskRef` for the line at `idx`, if it is a task line. A blank line is never addressed:
/// the daemon refuses a `TaskRef` to one (`specs/todotxt.abnf#blank`), and that refusal used to
/// end the whole TUI session (root todo: "J/K on or beside a blank line exits the TUI").
fn task_ref_at(state: &AppState, idx: usize) -> Option<pb::TaskRef> {
    let line = state.lines.get(idx)?;
    if line.raw.trim().is_empty() {
        return None;
    }
    Some(pb::TaskRef {
        line_number: line.line_number,
        task_id: line.task_ref_id().to_owned(),
    })
}

/// The index of the first task line at or after `from`, skipping blanks.
fn next_task_from(state: &AppState, from: usize) -> Option<usize> {
    (from..state.lines.len()).find(|&i| !state.lines[i].raw.trim().is_empty())
}

/// The index of the last task line at or before `from`, skipping blanks.
fn prev_task_from(state: &AppState, from: usize) -> Option<usize> {
    (0..=from)
        .rev()
        .find(|&i| !state.lines[i].raw.trim().is_empty())
}

/// `J`: builds the `MoveBefore`/`MoveToEnd` mutation that puts the selected task right after the
/// next task below it, blank lines skipped (a no-op on a blank line, the Add-a-line row or the
/// last task), then moves the cursor to where the task will land once the daemon's refresh does.
fn move_selected_down(state: &mut AppState) -> Option<pb::Mutation> {
    if state.on_add_line_row() {
        return None;
    }
    let task = task_ref_at(state, state.cursor)?;
    let next = next_task_from(state, state.cursor + 1)?;
    let (kind, lands_at) = match next_task_from(state, next + 1) {
        Some(after_next) => (
            pb::mutation::Kind::MoveBefore(pb::MoveBefore {
                task: Some(task),
                before: task_ref_at(state, after_next),
            }),
            after_next - 1,
        ),
        None => (
            pb::mutation::Kind::MoveToEnd(pb::MoveToEnd { task: Some(task) }),
            state.lines.len() - 1,
        ),
    };
    state.cursor = lands_at;
    Some(pb::Mutation { kind: Some(kind) })
}

/// `K`: builds the `MoveBefore` mutation that puts the selected task right before the previous
/// task above it, blank lines skipped (a no-op on a blank line, the first task or the Add-a-line
/// row), then moves the cursor to where the task will land.
fn move_selected_up(state: &mut AppState) -> Option<pb::Mutation> {
    if state.on_add_line_row() || state.cursor == 0 {
        return None;
    }
    let task = task_ref_at(state, state.cursor)?;
    let prev = prev_task_from(state, state.cursor - 1)?;
    let before = task_ref_at(state, prev)?;
    state.cursor = prev;
    Some(pb::Mutation {
        kind: Some(pb::mutation::Kind::MoveBefore(pb::MoveBefore {
            task: Some(task),
            before: Some(before),
        })),
    })
}

/// A row dragged from `from` and dropped on `to` (task `tui-revamp/tui-mouse`): the task lands
/// where `to` is, blank lines never addressed (`to` may be the Add-a-line row: the end). Nothing
/// when `from` is not a task or the drop would not move it. The cursor goes where it lands.
pub(crate) fn move_row(state: &mut AppState, from: usize, to: usize) -> Option<Action> {
    if under_review(state, from) {
        state.last_error = Some(READ_ONLY.to_owned());
        return None;
    }
    let task = Some(task_ref_at(state, from)?);
    let (kind, lands_at) = if to < from {
        let before = next_task_from(state, to).filter(|&b| b != from)?;
        let kind = pb::mutation::Kind::MoveBefore(pb::MoveBefore {
            task,
            before: task_ref_at(state, before),
        });
        (kind, before)
    } else {
        let last = state.lines.len().checked_sub(1)?;
        let anchor = prev_task_from(state, to.min(last)).filter(|&a| a != from)?;
        match next_task_from(state, anchor + 1) {
            Some(before) => (
                pb::mutation::Kind::MoveBefore(pb::MoveBefore {
                    task,
                    before: task_ref_at(state, before),
                }),
                before - 1,
            ),
            None => (pb::mutation::Kind::MoveToEnd(pb::MoveToEnd { task }), last),
        }
    };
    state.cursor = lands_at;
    apply(state, Some(pb::Mutation { kind: Some(kind) }))
}

/// Wraps one mutation as a single-mutation `ApplyRequest` against the open document.
pub(crate) fn apply_of(state: &AppState, mutation: pb::Mutation) -> pb::ApplyRequest {
    pb::ApplyRequest {
        workspace: None,
        path: state.path.clone(),
        mutations: vec![mutation],
        agent: None,
        // The activity log tells a TUI change from a CLI or desktop one (task op-source).
        source: "tui".to_owned(),
        dry_run: false,
    }
}

/// Today's date in the local calendar, `YYYY-MM-DD` (ADR 0011).
/// Ref: <https://docs.rs/jiff/latest/jiff/struct.Zoned.html>
pub(crate) fn today_local() -> String {
    jiff::Zoned::now().date().to_string()
}
