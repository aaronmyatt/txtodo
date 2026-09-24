//! The default merges only with an own device (task `default-workspace-pairing-consent`): a
//! session's route set keeps the reserved id for an own peer and swaps it for this device's alias
//! for any other peer, known or not.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use txtodo_model::{DeviceId, IdentityMode, Ulid};
use txtodo_store::NewDevice;

use crate::clock::FakeClock;
use crate::default_workspace::{default_alias, default_workspace_id};
use crate::device_relay::WorkspaceRoute;
use crate::lan_session_gate::session_routes;
use crate::workspace::Workspace;

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn register(ws: &Workspace, peer: DeviceId, own: bool) {
    let new = NewDevice {
        device: peer,
        name: String::new(),
        static_public: [1; 32],
        paired_at_ms: 1_000,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    ws.identity_store()
        .lock()
        .unwrap()
        .register_device_as(&new, own)
        .unwrap();
}

#[tokio::test]
async fn the_default_goes_by_its_alias_to_any_peer_that_is_not_own() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::open_with_default_mode(
        dir.path(),
        Arc::new(FakeClock::new(1_000)),
        IdentityMode::Sidecar,
    )
    .unwrap();
    let me = ws.device();
    register(&ws, device(2), true);
    register(&ws, device(3), false);
    let group = ws.group();
    let route = WorkspaceRoute {
        ws: Arc::new(RwLock::new(ws)),
        device: me,
        group,
    };
    let all = BTreeMap::from([(default_workspace_id(), route)]);

    let own = session_routes(&all, me, device(2));
    assert!(own.contains_key(&default_workspace_id()), "own: merges");

    for peer in [device(3), device(4)] {
        let apart = session_routes(&all, me, peer);
        assert!(!apart.contains_key(&default_workspace_id()), "{peer}");
        assert!(apart.contains_key(&default_alias(me)), "{peer}");
    }
}

#[test]
fn an_alias_is_stable_per_device_and_never_the_link_sentinel() {
    assert_eq!(default_alias(device(1)), default_alias(device(1)));
    assert_ne!(default_alias(device(1)), default_alias(device(2)));
    assert_ne!(default_alias(device(1)), default_workspace_id());
    assert_ne!(
        default_alias(device(1)).ulid().to_u128() >> 80,
        0,
        "a non-zero ULID timestamp"
    );
}
