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
    /// `o` pane `a`+`Enter`: adopt a peer's offered workspace at the typed directory.
    AcceptOffer(pb::WorkspaceAcceptOfferRequest),
    /// `o` pane `d`: discard a peer's offer.
    DeclineOffer(pb::WorkspaceDeclineOfferRequest),
    /// `:q`: exit the event loop.
    Quit,
}
