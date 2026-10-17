//! `m` / `c` on a line (specs/client-parity.toml list.move_to, list.copy_to; task
//! add-the-ability-to-move-or-copy-a-task-t): a popup of the workspace's other lists, the
//! desktop's Move to… / Copy to… picker. Enter on a list moves the line there (the daemon's
//! cross-file `Move`: the task keeps its id and its `ref:` folder goes too) or copies it (an `Add`
//! there, a new task without the `id:` and `ref:`). Pure; `app.rs` reads the lists first.

use txtodo_proto::v1 as pb;

use crate::action::Action;
use crate::commands::apply_of;
use crate::keymap::Command;
use crate::state::AppState;
use crate::state_nav::Overlay;

/// What Enter does with the picked list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickMode {
    /// `m`: the line leaves this list.
    Move,
    /// `c`: a new task there; this one stays.
    Copy,
}

/// The popup: the lists to pick from, the cursor, and what picking does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListPicker {
    /// Set while the popup is open.
    pub mode: Option<PickMode>,
    /// Workspace-relative list paths, the root list first.
    pub lists: Vec<String>,
    /// The selected row.
    pub cursor: usize,
}

/// Runs a picker command; `None` for any other command.
pub fn run(state: &mut AppState, command: Command) -> Option<Option<Action>> {
    let picker = &mut state.shell.picker;
    match command {
        Command::ListMoveTo => return Some(open_action(state, PickMode::Move)),
        Command::ListCopyTo => return Some(open_action(state, PickMode::Copy)),
        Command::ListPickerDown => {
            picker.cursor = (picker.cursor + 1).min(picker.lists.len().saturating_sub(1));
        }
        Command::ListPickerUp => picker.cursor = picker.cursor.saturating_sub(1),
        Command::ListPickerClose => state.nav.overlay = None,
        Command::ListPickerOpen => {
            state.nav.overlay = None;
            return Some(pick(state));
        }
        _ => return None,
    }
    Some(None)
}

/// The lists are the daemon's to say: `app` reads them, then calls [`open`].
fn open_action(state: &AppState, mode: PickMode) -> Option<Action> {
    state.selected_line().filter(|l| !l.raw.trim().is_empty())?;
    Some(Action::OpenListPicker(mode))
}

/// Opens the popup over every list of the workspace but this one, or says there is none.
pub fn open(state: &mut AppState, mode: PickMode, files: &[pb::FileInfo]) {
    let lists = other_lists(files, &state.path);
    if lists.is_empty() {
        state.last_error = Some("no other list in this workspace".to_owned());
        return;
    }
    state.shell.picker = ListPicker {
        mode: Some(mode),
        lists,
        cursor: 0,
    };
    state.nav.overlay = Some(Overlay::ListPicker);
}

/// Enter: the `Apply` that moves or copies the selected line to the picked list.
fn pick(state: &mut AppState) -> Option<Action> {
    let dest = state
        .shell
        .picker
        .lists
        .get(state.shell.picker.cursor)?
        .clone();
    let mode = state.shell.picker.mode.take()?;
    let line = state.selected_line()?.clone();
    let (req, toast) = match mode {
        PickMode::Move => {
            let task = Some(pb::TaskRef {
                line_number: line.line_number,
                task_id: line.task_ref_id().to_owned(),
            });
            let kind = pb::mutation::Kind::Move(pb::Move {
                task,
                to_path: dest.clone(),
            });
            // `u` would take back only this list's half of a move.
            state.shell.pending_no_undo = true;
            let req = apply_of(state, pb::Mutation { kind: Some(kind) });
            (req, format!("Moved to {dest}"))
        }
        PickMode::Copy => {
            let line = copy_text(&line.raw);
            let kind = pb::mutation::Kind::Add(pb::Add { line });
            let mut req = apply_of(state, pb::Mutation { kind: Some(kind) });
            req.path = dest.clone();
            (req, format!("Copied to {dest}"))
        }
    };
    state.shell.pending_toast = Some(toast);
    Some(Action::Apply(req))
}

/// Every list in `files` but `from`, by its path: the root list first, then by depth and name.
/// A notes file is not a list.
pub fn other_lists(files: &[pb::FileInfo], from: &str) -> Vec<String> {
    let notes = pb::FileKind::Notes as i32;
    let mut lists: Vec<String> = files
        .iter()
        .filter(|f| f.kind != notes && f.path.ends_with(".txt") && f.path != from)
        .map(|f| f.path.clone())
        .collect();
    lists.sort_by_key(|p| (p.matches('/').count(), p.clone()));
    lists
}

/// A copy is a new task: the line without its `id:` (the daemon mints one) and its `ref:` (the
/// notes and sub-list stay with the original).
pub fn copy_text(raw: &str) -> String {
    let tag = |w: &str| {
        ["id:", "ref:"]
            .iter()
            .any(|k| w.len() > k.len() && w.starts_with(k))
    };
    raw.split(' ')
        .filter(|w| !tag(w))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "list_picker_tests.rs"]
mod tests;
