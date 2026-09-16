//! `record_offer_relay_reachability` (task `daemon-workspace-identity-agreement` stage 2): a
//! `PairingOffer`'s relay fields, when present, land durably against the offering device's row;
//! a LAN-only offer (`None`/`None`) is a harmless no-op, not an error.

use std::sync::{Arc, RwLock};

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::NewDevice;
use txtodo_sync::{GroupId, InitiatorReply, JoinerHello, PairingOffer};

use crate::clock::{Clock, FakeClock};
use crate::pairing_lan::{process_hello, record_offer_relay_reachability};
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

fn hello(device_id: DeviceId, group: GroupId, nonce: [u8; 16]) -> JoinerHello {
    JoinerHello {
        device: device_id,
        group,
        nonce,
        public_key: [0xCD; 32],
        static_public: [0xEF; 32],
        confirmed: false,
    }
}

/// `pairing_lan.rs`'s own former bug (root todo `logging-daemon-swallowed-errors`): every
/// `InitiatorReply::Rejected` return used to be completely invisible on the initiator side. This
/// asserts the wire behaviour is unchanged now that each reason is logged — a fresh workspace has
/// no active pairing at all, so any hello is rejected.
#[test]
fn process_hello_rejects_when_no_pairing_is_active() {
    let ws = shared_workspace();
    let reply = process_hello(&ws, hello(device(1), GroupId(1), [1u8; 16]));
    assert!(matches!(reply, InitiatorReply::Rejected));
}

/// A hello whose group/nonce does not match the initiator's own active offer is rejected too —
/// distinct from the "nothing active at all" case above, but previously just as invisible.
#[test]
fn process_hello_rejects_a_group_or_nonce_mismatch() {
    let ws = shared_workspace();
    let offer = ws
        .read()
        .unwrap()
        .pairing()
        .begin_offer(device(0), GroupId(1), "192.168.1.5:4242".to_string(), 1_000)
        .unwrap();

    let wrong_group = process_hello(&ws, hello(device(1), GroupId(2), offer.nonce));
    assert!(matches!(wrong_group, InitiatorReply::Rejected));

    let wrong_nonce = process_hello(&ws, hello(device(1), offer.group, [0u8; 16]));
    assert!(matches!(wrong_nonce, InitiatorReply::Rejected));
}
