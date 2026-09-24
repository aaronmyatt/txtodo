//! The per-device carriers besides the relay endpoint itself, split out of `main.rs` for its line
//! budget: the relay control channel, the shared file-carrier and the one LAN transport.

use std::path::PathBuf;
use std::sync::Arc;

use txtodo_daemon::device_identity::DeviceIdentity;
use txtodo_daemon::device_relay::DeviceRelay;
use txtodo_daemon::file_carrier::DeviceFileCarrier;

use crate::Args;

/// What every carrier here is built from, bundled to stay under `maxParams`.
pub(crate) struct Deps {
    pub(crate) identity: Arc<DeviceIdentity>,
    pub(crate) device_relay: Option<Arc<DeviceRelay>>,
    pub(crate) clock: Arc<dyn txtodo_daemon::clock::Clock>,
    pub(crate) registry_path: PathBuf,
}

/// The per-device carriers, started before any workspace opens: the relay control channel (when
/// a relay is bound), one shared file-carrier (stage 6) and one LAN transport (task
/// sync-live-push), the last off like relay without a keystore that can keep keys. Kept alive for
/// `run`'s whole life.
pub(crate) struct Carriers {
    _control: Option<txtodo_daemon::control_channel::ControlChannelTransport>,
    pub(crate) file: Option<Arc<DeviceFileCarrier>>,
    _file_task: Option<txtodo_daemon::file_carrier::FileCarrierTransport>,
    pub(crate) lan: Option<(
        Arc<txtodo_daemon::device_lan::DeviceLan>,
        txtodo_daemon::lan::LanTransport,
    )>,
}

pub(crate) fn start(args: &Args, sync_allowed: bool, deps: Deps) -> Carriers {
    let Deps {
        identity,
        device_relay,
        clock,
        registry_path,
    } = deps;
    let file = args
        .sync_dir
        .clone()
        .and_then(|dir| DeviceFileCarrier::open(dir, identity.device()));
    Carriers {
        _control: txtodo_daemon::control_channel::start(
            Arc::clone(&identity),
            device_relay.clone(),
            registry_path.clone(),
        ),
        _file_task: file.clone().map(txtodo_daemon::file_carrier::start),
        file,
        lan: txtodo_daemon::device_lan::start(
            !args.no_lan && sync_allowed,
            identity,
            device_relay,
            clock,
            txtodo_daemon::device_lan::open_registry(&registry_path),
        ),
    }
}
