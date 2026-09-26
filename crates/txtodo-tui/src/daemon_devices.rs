//! Pairing and paired devices (task `tui-revamp/tui-foundation`), for Settings › Devices. Split
//! out of `daemon.rs` by area. The SAS confirm is the human's call: nothing here confirms on its
//! own.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};

impl Daemon {
    /// Starts a pairing as the initiator: the code (and QR payload) to show.
    pub async fn pair_offer(&mut self) -> Result<pb::PairOfferResponse, DaemonError> {
        let req = pb::PairOfferRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.pair_offer(req).await?.into_inner())
    }

    /// Joins with another device's code; the result carries the six SAS words to compare. Sent
    /// with no workspace, so the daemon answers for the default, which it never rekeys
    /// (sync-drift line 4): the selected folder stays as it is, and the other device's
    /// workspaces arrive as Remote mirrors in fresh folders. Naming the selected workspace would
    /// ask the daemon to join it in place, which it refuses once that folder has lines, and the
    /// TUI started in a folder with a `todo.txt` selects that folder.
    pub async fn pair_accept(&mut self, code: &str) -> Result<pb::PairResult, DaemonError> {
        let req = pb::PairAcceptRequest {
            code: code.to_owned(),
            workspace: None,
        };
        Ok(self.inner.pair_accept(req).await?.into_inner())
    }

    /// The initiator's poll: an empty `sas` means no joiner has arrived yet.
    pub async fn pair_await_peer(&mut self) -> Result<pb::PairResult, DaemonError> {
        let req = pb::PairAwaitPeerRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.pair_await_peer(req).await?.into_inner())
    }

    /// The human said the words match; `own_device` is their answer to "is it yours?".
    pub async fn pair_confirm_sas(
        &mut self,
        own_device: bool,
    ) -> Result<pb::PairResult, DaemonError> {
        let req = pb::PairConfirmRequest {
            workspace: self.selector.clone(),
            own_device,
        };
        Ok(self.inner.pair_confirm_sas(req).await?.into_inner())
    }

    /// Every paired device, with its clock skew.
    pub async fn device_list(&mut self) -> Result<pb::DeviceListResponse, DaemonError> {
        let req = pb::DeviceListRequest {
            workspace: self.selector.clone(),
        };
        Ok(self.inner.device_list(req).await?.into_inner())
    }

    /// Revokes a device, which rotates the group key; the response says what did not un-share.
    pub async fn device_remove(
        &mut self,
        id: &str,
    ) -> Result<pb::DeviceRemoveResponse, DaemonError> {
        let req = pb::DeviceRemoveRequest {
            id: id.to_owned(),
            workspace: self.selector.clone(),
        };
        Ok(self.inner.device_remove(req).await?.into_inner())
    }
}
