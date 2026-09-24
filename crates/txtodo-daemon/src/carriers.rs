//! The per-device carriers besides relay, split out of `main.rs` for its line budget.

use std::sync::Arc;

use txtodo_daemon::device_identity::DeviceIdentity;
use txtodo_daemon::device_relay::DeviceRelay;
use txtodo_daemon::file_carrier::DeviceFileCarrier;

use crate::Args;

/// The per-device carriers besides relay, started before any workspace opens: one shared
/// file-carrier (stage 6) and one LAN transport (task sync-live-push), off like relay without a
/// keystore that can keep keys. Kept alive for `run`'s whole life.
pub(crate) struct Carriers {
    pub(crate) file: Option<Arc<DeviceFileCarrier>>,
    _file_task: Option<txtodo_daemon::file_carrier::FileCarrierTransport>,
    pub(crate) lan: Option<(
        Arc<txtodo_daemon::device_lan::DeviceLan>,
        txtodo_daemon::lan::LanTransport,
    )>,
}

pub(crate) fn start(
    args: &Args,
    identity: &Arc<DeviceIdentity>,
    device_relay: &Option<Arc<DeviceRelay>>,
    sync_allowed: bool,
    clock: Arc<dyn txtodo_daemon::clock::Clock>,
) -> Carriers {
    let file = args
        .sync_dir
        .clone()
        .and_then(|dir| DeviceFileCarrier::open(dir, identity.device()));
    Carriers {
        _file_task: file.clone().map(txtodo_daemon::file_carrier::start),
        file,
        lan: txtodo_daemon::device_lan::start(
            !args.no_lan && sync_allowed,
            Arc::clone(identity),
            device_relay.clone(),
            clock,
        ),
    }
}
