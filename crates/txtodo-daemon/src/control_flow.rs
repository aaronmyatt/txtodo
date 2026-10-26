//! The order of one control-channel session (task `peer-prune`, ADR 0044), now that the group key
//! can rotate and a peer may be on another epoch.
//!
//! A frame is sealed under one epoch and a device can open only the epochs it holds, so a device
//! that sent its offers the moment the link opened would hand a peer one epoch behind a frame it
//! cannot read, and the peer would end the session before any grant arrived. So:
//!
//! 1. The **dialer** speaks first, with `Epoch(its epoch)` sealed under the lower of its own epoch
//!    and what it believes the peer holds. The **acceptor** sends nothing until it has read a frame.
//! 2. On the first `Epoch` from a peer: the acceptor answers with its own, sealed under the lower of
//!    the two epochs; a device that is ahead sends the grants in between; then both send their
//!    offers and own-device list, under their current epoch (the peer holds it now).
//! 3. A `Grant` is applied (`group_rotation.rs`) and answered with `Epoch`, which is how the sender
//!    learns the peer caught up.
//!
//! A frame from a device this one removed ends the session unread: a pruned device may still hold
//! an old key, and opens this channel with it.

use std::sync::Mutex;

use txtodo_model::DeviceId;
use txtodo_sync::{ControlMessage, GroupId, GroupKeys, Link};

use crate::control_session::{handle_one_message, now_ms, recv_control, send_all_offers};
use crate::device_identity::DeviceIdentity;
use crate::group_rotation::{self, Applied};
use crate::peer_keys::PeerSignal;
use crate::workspace_registry::WorkspaceRegistry;

/// One session's state. `keys` holds every epoch this device holds up to `epoch`.
pub(crate) struct Control<'a> {
    pub(crate) link: &'a mut dyn Link,
    pub(crate) identity: &'a DeviceIdentity,
    pub(crate) registry: &'a Mutex<WorkspaceRegistry>,
    pub(crate) group: GroupId,
    pub(crate) keys: GroupKeys,
    pub(crate) epoch: u32,
    /// Who this session dialed; `None` when it accepted.
    pub(crate) dialed: Option<DeviceId>,
    introduced: bool,
}

impl<'a> Control<'a> {
    pub(crate) fn new(
        link: &'a mut dyn Link,
        identity: &'a DeviceIdentity,
        registry: &'a Mutex<WorkspaceRegistry>,
        keys: GroupKeys,
        dialed: Option<DeviceId>,
    ) -> Control<'a> {
        Control {
            group: identity.group(),
            epoch: identity.group_epoch(),
            link,
            identity,
            registry,
            keys,
            dialed,
            introduced: false,
        }
    }

    /// Seals `msg` under `under` (an epoch this device holds) and sends it; `None` once the link
    /// is gone. A message that cannot be sealed is logged and skipped.
    pub(crate) fn send(&mut self, under: u32, msg: ControlMessage) -> Option<()> {
        let Some(key) = self.keys.get(under) else {
            log_no_key_to_seal(under);
            return Some(());
        };
        match txtodo_sync::seal_control(&msg, self.group, under, key) {
            Ok(frame) => self.link.send(frame).ok(),
            Err(e) => log_seal_failed(&e),
        }
    }

    fn announce(&mut self, under: u32) -> Option<()> {
        let msg = ControlMessage::Epoch {
            sender: self.identity.device(),
            epoch: self.epoch,
        };
        self.send(under, msg)
    }

    /// The dialer's first word. `false` once the link is gone.
    pub(crate) fn start(&mut self) -> bool {
        let Some(peer) = self.dialed else {
            return true;
        };
        let believed = group_rotation::believed_epoch(self.identity, peer).unwrap_or(self.epoch);
        let under = if self.keys.get(believed).is_some() {
            self.epoch.min(believed)
        } else {
            self.epoch
        };
        self.announce(under).is_some()
    }

    /// Reads and handles frames until the link closes (`Opened` if any came) or one does not open.
    pub(crate) fn run(&mut self) -> PeerSignal {
        let mut seen = PeerSignal::Silent;
        loop {
            match recv_control(self.link, self.group, &self.keys) {
                Ok(msg) => {
                    seen = PeerSignal::Opened;
                    if !self.on_message(msg) {
                        return seen;
                    }
                }
                Err(PeerSignal::Silent) => return seen,
                Err(failed) => return failed,
            }
        }
    }

    /// `false` ends the session.
    fn on_message(&mut self, msg: ControlMessage) -> bool {
        let sender = crate::own_vouch::sender_of(&msg);
        if group_rotation::is_removed(self.identity, sender) {
            log_removed_peer(sender);
            return false;
        }
        match msg {
            ControlMessage::Epoch { sender, epoch } => self.on_epoch(sender, epoch).is_some(),
            ControlMessage::Grant { .. } => self.on_grant(&msg).is_some(),
            other => {
                handle_one_message(self.identity, other);
                true
            }
        }
    }

    fn on_epoch(&mut self, peer: DeviceId, peer_epoch: u32) -> Option<()> {
        group_rotation::note_peer_epoch(self.identity, peer, peer_epoch);
        if self.introduced {
            return Some(());
        }
        self.introduced = true;
        if self.dialed.is_none() {
            self.announce(self.epoch.min(peer_epoch))?;
        }
        for grant in group_rotation::grants_for(self.identity, peer, peer_epoch) {
            self.send(grant.sealed_under, grant.message)?;
        }
        self.send_offers_and_vouch(peer)
    }

    /// Offers and the own-device list, under this device's current epoch.
    fn send_offers_and_vouch(&mut self, peer: DeviceId) -> Option<()> {
        let key = self.keys.get(self.epoch)?;
        let ctx = crate::control_session::SealCtx {
            group: self.group,
            epoch: self.epoch,
            key,
        };
        if !send_all_offers(self.link, self.identity, self.registry, &ctx) {
            return None;
        }
        if let Some(own) = crate::own_vouch::own_devices_for(self.identity, peer) {
            self.send(self.epoch, own)?;
        }
        Some(())
    }

    fn on_grant(&mut self, msg: &ControlMessage) -> Option<()> {
        match group_rotation::apply_grant(self.identity, msg, now_ms()) {
            Applied::Adopted { epoch } => {
                self.refresh();
                self.announce(epoch)
            }
            Applied::Kept => Some(()),
            Applied::Ignored(why) => log_grant_ignored(why),
        }
    }

    /// Reloads the epoch and the keys after a grant moved them.
    fn refresh(&mut self) {
        self.epoch = self.identity.group_epoch();
        if let Some(keys) =
            crate::group_epoch::retained(self.identity.key_store().as_ref(), self.epoch)
        {
            self.keys = keys;
        }
    }
}

fn log_no_key_to_seal(epoch: u32) {
    tracing::debug!(epoch, "control_channel_no_key_to_seal");
}

fn log_seal_failed(e: &txtodo_sync::ControlSealError) -> Option<()> {
    tracing::debug!(error = %e, "control_channel_seal_failed");
    Some(())
}

fn log_removed_peer(peer: DeviceId) {
    tracing::info!(%peer, "control_channel_removed_peer_refused");
}

fn log_grant_ignored(why: &'static str) -> Option<()> {
    tracing::info!(why, "control_channel_grant_ignored");
    Some(())
}
