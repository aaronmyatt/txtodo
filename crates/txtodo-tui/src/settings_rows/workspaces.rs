//! The rows of Settings › Workspaces (task `tui-revamp/tui-settings`; kinds and the New list row,
//! task two-kinds-of-workspace). Split out of `settings_rows.rs` for its line budget.

use super::{Act, SRow, field, not_here};
use crate::state::AppState;
use crate::state_settings::WsRow;
use crate::state_shell::WsKind;

pub(super) fn rows(state: &AppState) -> Vec<SRow> {
    // Lists first (the default among them), repos after, as the `W` popup has them.
    let mut listed: Vec<&WsRow> = state.settings.workspaces.iter().collect();
    listed.sort_by_key(|w| w.kind == WsKind::Repo);
    let mut out: Vec<SRow> = listed
        .into_iter()
        .map(|w| {
            let mut label = w.name.clone();
            if w.current {
                label.push_str(" \u{b7} current");
            }
            let count = match (w.missing, w.open) {
                (true, _) => "missing".to_owned(),
                (false, Some(n)) => format!("{n} open"),
                (false, None) => "not loaded".to_owned(),
            };
            let kind = match w.kind.label() {
                "" => String::new(),
                label => format!(" \u{b7} {label}"),
            };
            let removable = !w.current && !w.is_default;
            SRow {
                label,
                value: format!("{} \u{b7} {count}{kind}", w.root),
                act: Some(Act::OpenWorkspace(w.id.clone())),
                remove: removable.then(|| Act::RemoveWorkspace(w.id.clone())),
                ..SRow::default()
            }
        })
        .collect();
    // The open one only: the header names it, and the field starts from its name. Another
    // workspace is renamed from there (manifest `workspace.rename`).
    if let Some(w) = state.settings.workspaces.iter().find(|w| w.current) {
        out.push(field(
            "Rename the open one",
            "the name every paired device shows; empty clears it",
            Act::RenameWorkspace(w.id.clone()),
        ));
    }
    out.push(field(
        "New list",
        "type a name, Enter makes a list your devices share",
        Act::NewList,
    ));
    out.push(field(
        "Add a workspace",
        "type a folder path, Enter adds it",
        Act::AddWorkspace,
    ));
    out.push(not_here(
        "Folder picker",
        "A terminal has no folder picker: type the path.",
    ));
    out
}
