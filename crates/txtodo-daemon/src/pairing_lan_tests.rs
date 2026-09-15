//! `record_offer_relay_reachability` (task `daemon-workspace-identity-agreement` stage 2): a
//! `PairingOffer`'s relay fields, when present, land durably against the offering device's row;
//! a LAN-only offer (`None`/`None`) is a harmless no-op, not an error.

use std::sync::{Arc, RwLock};

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::NewDevice;
use txtodo_sync::{GroupId, PairingOffer};

use crate::clock::{Clock, FakeClock};
use crate::pairing_lan::record_offer_relay_reachability;
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;

fn shared_workspace() -> SharedWorkspace {
    let dir = tempfile::tempdir().unwrap();
    let dir = Box::leak(Box::new(dir));
    let ws = Workspace::open(
        dir.path(),
        Arc::new(FakeClock::new(1_000)) as Arc<dyn Clock>,
    )
    .unwrap_or_else(|e| panic!("open: {e}"));
    Arc::new(RwLock::new(ws))
}

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn register_peer(ws: &SharedWorkspace, n: u128) {
    ws.read()
        .unwrap()
        .identity_store()
        .lock()
        .unwrap()
        .register_device(&NewDevice {
            device: device(n),
            name: format!("peer-{n}"),
            static_public: [n as u8; 32],
            paired_at_ms: 1_000,
            last_known_wall_ms: None,
            key_epoch: 0,
        })
        .unwrap();
}

fn offer(
    device_id: DeviceId,
    relay_node_id: Option<[u8; 32]>,
    relay_url: Option<&str>,
) -> PairingOffer {
    PairingOffer {
        device: device_id,
        group: GroupId(1),
        public_key: [0xAB; 32],
        endpoint: "192.168.1.5:4242".to_string(),
        nonce: [7u8; 16],
        issued_at_ms: 1_000,
        relay_node_id,
        relay_url: relay_url.map(str::to_string),
    }
}

#[test]
fn a_relay_offer_lands_the_peers_reachability() {
    let ws = shared_workspace();
    register_peer(&ws, 1);
    let node_id = [9u8; 32];
    record_offer_relay_reachability(
        &ws,
        &offer(device(1), Some(node_id), Some("https://relay.example.org")),
    );

    let row = ws
        .read()
        .unwrap()
        .identity_store()
        .lock()
        .unwrap()
        .device(device(1))
        .unwrap()
        .unwrap();
    assert_eq!(row.relay_node_id, Some(node_id));
    assert_eq!(row.relay_url.as_deref(), Some("https://relay.example.org"));
}

#[test]
fn a_lan_only_offer_is_a_harmless_no_op() {
    let ws = shared_workspace();
    register_peer(&ws, 1);
    record_offer_relay_reachability(&ws, &offer(device(1), None, None));

    let row = ws
        .read()
        .unwrap()
        .identity_store()
        .lock()
        .unwrap()
        .device(device(1))
        .unwrap()
        .unwrap();
    assert_eq!(row.relay_node_id, None);
    assert_eq!(row.relay_url, None);
}
