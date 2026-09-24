//! The [`Action`] an [`crate::input::Input::on_key`] dispatch decided on. Kept as its own module
//! (rather than nested in `input.rs` or `app.rs`) since both depend on it: `input.rs` builds one,
//! `app.rs::perform` is the only place that actually sends it to the daemon.

use txtodo_proto::v1 as pb;

/// A side effect a key dispatch decided on; only `app::perform` actually performs it, so the
/// dispatch logic itself needs no daemon and no I/O to test.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Send `ApplyRequest` and, on success, re-baseline from the reply.
    Apply(pb::ApplyRequest),
    /// Send `ResolveConflict`.
    Resolve(pb::ResolveRequest),
    /// `o` pane `a`: accept a peer's offered workspace; the daemon mirrors it into a folder of its
    /// own choosing (task `remote-workspace-mirror`), so nothing is typed.
    AcceptOffer(pb::WorkspaceAcceptOfferRequest),
    /// `o` pane `d`: discard a peer's offer.
    DeclineOffer(pb::WorkspaceDeclineOfferRequest),
    /// `:w <workspace>`: switch to the workspace a name, id or path picks (task
    /// `tui-revamp/tui-foundation`).
    SwitchWorkspace(String),
    /// `W`: list the workspaces and their open counts, then open the popup (task
    /// `tui-revamp/tui-shell`).
    OpenWorkspaceMenu,
    /// Put this text on the clipboard (OSC 52): a refused edit, a new token's secret.
    Copy(String),
    /// `u` or a toast's Undo: the daemon's `Undo` of the newest ops (this many) to this document,
    /// in this workspace (by id; `None` for the open one).
    Undo(String, u32, Option<String>),
    /// Enter on a line: open it in the detail panel (task `tui-revamp/tui-detail`).
    OpenDetail(crate::state_detail::Parent),
    /// Save a level's notes (`EditNotes`): its parent's `TaskRef` and the whole text.
    SaveNotes(pb::TaskRef, String),
    /// Re-read the Universal screen's rows.
    RefreshUniversal,
    /// Universal's Enter: open the task on its line in its workspace.
    OpenUniversal(crate::state_universal::UTask),
    /// Universal's `x`: complete the task in its workspace.
    CompleteUniversal(crate::state_universal::UTask),
    /// `:q`: exit the event loop.
    Quit,
}
