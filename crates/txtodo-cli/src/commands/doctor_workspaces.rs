//! `txtodo doctor`'s `workspace` rows for every registered workspace but the one it runs in
//! (ADR 0025, task `cli-doctor-multi-workspace`), split out of `doctor.rs` for its file budget.

use std::path::Path;

use super::doctor::{Check, Status, check};
use crate::client;
use txtodo_proto::v1 as pb;

/// One line per *other* registered workspace (ADR 0025, task `cli-doctor-multi-workspace`) — the
/// seven fixed checks above already cover the current one in full depth, so this stays a cheap
/// per-entry `Health` probe, not a second full battery of checks. Best-effort: a legacy
/// `--dir`-bridge daemon has no registry, so an empty `workspaces`
/// (`doctor_overlap::registered_workspaces`) just means there is nothing more to report, not a
/// doctor failure — `run`'s existing seven checks already told the human that story if it matters.
pub(super) fn other_workspace_checks(
    daemon: Option<&mut client::Daemon>,
    workspaces: &[pb::WorkspaceInfo],
    current: &Path,
) -> Vec<Check> {
    let Some(daemon) = daemon else {
        return Vec::new();
    };
    let current = current
        .canonicalize()
        .unwrap_or_else(|_| current.to_owned());
    workspaces
        .iter()
        .filter(|w| Path::new(&w.root) != current)
        .map(|w| {
            let label = format!("{}  {}", w.workspace_id, w.root);
            match daemon.health_for_id(&w.workspace_id) {
                Ok(h) => check(
                    "workspace",
                    Status::Ok,
                    format!("{label}: {} document(s)", h.documents),
                ),
                Err(e) => check("workspace", Status::Warn, format!("{label}: {e}")),
            }
        })
        .collect()
}
