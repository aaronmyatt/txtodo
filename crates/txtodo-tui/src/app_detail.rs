//! The detail panel's daemon calls (task `tui-revamp/tui-detail`): opening a line (its `ref:`
//! directory through `RefDir`, the sub-list through `GetFile`, the notes through `GetNotes`),
//! re-reading whichever open list a change names, making the directory before a first sub-task
//! (`RefDir(ensure)`, which also tags the parent `ref:`), and saving notes (`EditNotes`).
//! A refusal goes on the status line; a transport error ends the call like any other.

use txtodo_proto::v1 as pb;

use crate::app::rebaseline;
use crate::daemon::{Daemon, DaemonError};
use crate::state::{AppState, LineState};
use crate::state_detail::{Doc, Level, Notes, Parent, Part};
use crate::state_nav::Focus;

/// Opens `parent` as a level: on top of the level whose sub-list it is in, or, from the root
/// list, as the only one. Unsaved notes of the levels it replaces are saved first.
pub async fn open(
    daemon: &mut Daemon,
    state: &mut AppState,
    mut parent: Parent,
) -> Result<(), DaemonError> {
    let keep = state
        .detail
        .levels
        .iter()
        .position(|l| l.doc.path == parent.path)
        .map_or(0, |i| i + 1);
    save_dirty_notes(daemon, state).await?;
    state.detail.levels.truncate(keep);
    let task = task_ref(&parent);
    let info = match daemon.ref_dir(&parent.path, task, false).await {
        Ok(info) => info,
        Err(DaemonError::Rpc(status)) => {
            state.last_error = Some(status.message().to_owned());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    parent.task_id = info.task_id;
    let path = format!("{}/todo.txt", info.dir);
    let lines = read_lines(daemon, &path).await?;
    let text = read_notes(daemon, &parent).await?;
    state.detail.levels.push(Level {
        parent,
        dir: info.dir,
        dir_exists: info.dir_exists,
        doc: Doc {
            path,
            lines: lines.unwrap_or_default(),
            ..Doc::default()
        },
        notes: Notes {
            saved: text.clone(),
            text,
            ..Notes::default()
        },
        parent_draft: None,
    });
    state.detail.part = Part::Sub;
    state.nav.focus = Focus::Detail;
    state.rewatch = true;
    Ok(())
}

/// The `TaskRef` for a level's parent line.
pub fn task_ref(parent: &Parent) -> pb::TaskRef {
    pb::TaskRef {
        line_number: parent.line_number,
        task_id: parent.task_id.clone(),
    }
}

/// A list's lines, or `None` when the daemon has no such document (a `ref:` folder with no
/// `todo.txt` yet).
async fn read_lines(
    daemon: &mut Daemon,
    path: &str,
) -> Result<Option<Vec<LineState>>, DaemonError> {
    match daemon.get_file(path).await {
        Ok(file) => {
            let text = String::from_utf8_lossy(&file.bytes);
            Ok(Some(AppState::from_document(path, &text).lines))
        }
        Err(DaemonError::Rpc(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The parent's `notes.md`; empty when it has none, or the daemon refuses.
async fn read_notes(daemon: &mut Daemon, parent: &Parent) -> Result<String, DaemonError> {
    match daemon.get_notes(task_ref(parent)).await {
        Ok(doc) => Ok(String::from_utf8_lossy(&doc.bytes).into_owned()),
        Err(DaemonError::Rpc(_)) => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Re-reads the open list at `path` (the root list or a level's sub-list), then each level's
/// parent line from the list it lives in.
pub async fn refetch(
    daemon: &mut Daemon,
    state: &mut AppState,
    path: &str,
) -> Result<(), DaemonError> {
    if path == state.path {
        let file = daemon.get_file(path).await?;
        rebaseline(state, &file);
    } else if state.detail.levels.iter().any(|l| l.doc.path == path) {
        let lines = read_lines(daemon, path).await?.unwrap_or_default();
        for level in state
            .detail
            .levels
            .iter_mut()
            .filter(|l| l.doc.path == path)
        {
            level.doc.cursor = level.doc.cursor.min(lines.len());
            level.doc.lines.clone_from(&lines);
        }
    }
    refresh_parents(state);
    Ok(())
}

/// Each level's parent text and done state, from the list it lives in (found by task id, else
/// by line number), so an edit to a parent shows in the panel.
fn refresh_parents(state: &mut AppState) {
    let mut above: Vec<LineState> = state.lines.clone();
    for level in &mut state.detail.levels {
        let parent = &mut level.parent;
        let found = above
            .iter()
            .find(|l| !parent.task_id.is_empty() && l.task_ref_id() == parent.task_id)
            .or_else(|| above.iter().find(|l| l.line_number == parent.line_number));
        if let Some(line) = found {
            parent.raw.clone_from(&line.raw);
            parent.completed = line.completed;
            parent.line_number = line.line_number;
        }
        above.clone_from(&level.doc.lines);
    }
}

/// Before the first `Apply` to a level's sub-list: makes its `ref:` directory (and tags the
/// parent), as desktop's first sub-task does. A no-op for any other path.
pub async fn ensure_sub_list(
    daemon: &mut Daemon,
    state: &mut AppState,
    path: &str,
) -> Result<(), DaemonError> {
    let Some(i) = state
        .detail
        .levels
        .iter()
        .position(|l| l.doc.path == path && !l.dir_exists)
    else {
        return Ok(());
    };
    let parent = state.detail.levels[i].parent.clone();
    let info = daemon
        .ref_dir(&parent.path, task_ref(&parent), true)
        .await?;
    let level = &mut state.detail.levels[i];
    level.dir_exists = info.dir_exists;
    level.parent.task_id = info.task_id;
    // The parent's list gained a `ref:` tag.
    refetch(daemon, state, &parent.path).await
}

/// Saves `text` as the notes of the level whose parent is `task`.
pub async fn save_notes(
    daemon: &mut Daemon,
    state: &mut AppState,
    task: pb::TaskRef,
    text: String,
) -> Result<(), DaemonError> {
    match daemon.edit_notes(task.clone(), &text).await {
        Ok(_) => {
            for level in &mut state.detail.levels {
                if level.parent.task_id == task.task_id {
                    level.notes.saved.clone_from(&text);
                    level.dir_exists = true;
                }
            }
            Ok(())
        }
        Err(DaemonError::Rpc(status)) => {
            state.last_error = Some(status.message().to_owned());
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Saves the shown level's notes if they hold unsaved text.
pub async fn save_dirty_notes(
    daemon: &mut Daemon,
    state: &mut AppState,
) -> Result<(), DaemonError> {
    let Some(level) = state.detail.top().filter(|l| l.notes.dirty()) else {
        return Ok(());
    };
    let (task, text) = (task_ref(&level.parent), level.notes.text.clone());
    save_notes(daemon, state, task, text).await
}

/// When the shown level's notes are due to save: [`AUTOSAVE_AFTER`] past the last key, while they
/// hold unsaved text.
pub fn autosave_due(state: &AppState) -> Option<std::time::Instant> {
    let notes = &state.detail.top()?.notes;
    let typed = notes.typed_at?;
    notes.dirty().then(|| typed + AUTOSAVE_AFTER)
}

/// How long typing must pause before the notes save (desktop's `NotesEditor`).
pub const AUTOSAVE_AFTER: std::time::Duration = std::time::Duration::from_millis(500);

/// The documents the loop watches: the root list and each open sub-list the daemon has.
pub fn watched_paths(state: &AppState) -> Vec<String> {
    let mut paths = vec![state.path.clone()];
    paths.extend(
        state
            .detail
            .levels
            .iter()
            .filter(|l| !l.doc.lines.is_empty())
            .map(|l| l.doc.path.clone()),
    );
    paths
}
