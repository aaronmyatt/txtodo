//! The default and a named list are private to the user's own devices (tasks
//! `default-workspace-pairing-consent`, `two-kinds-of-workspace`): a session's route set keeps both
//! for an own peer and carries neither to any other peer, known or not. The default used to reach a
//! foreign peer under an alias; that went with the Remote mirror.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use txtodo_model::{DeviceId, IdentityMode, Ulid};
use txtodo_store::NewDevice;

use crate::clock::FakeClock;
use crate::default_workspace::default_workspace_id;
use crate::device_relay::WorkspaceRoute;
use crate::lan_session_gate::session_routes;
use crate::list_workspace::ListName;
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
async fn the_default_and_a_named_list_go_to_an_own_peer_and_to_nobody_else() {
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
    let work = ListName::parse("work").unwrap().id();
    let all = BTreeMap::from([
        (default_workspace_id(), route.clone()),
        (work, route.clone()),
    ]);

    let own = session_routes(&all, device(2));
    assert!(own.contains_key(&work), "an own device gets the list");
    assert!(own.contains_key(&default_workspace_id()), "and the default");

    for peer in [device(3), device(4)] {
        let apart = session_routes(&all, peer);
        assert!(
            apart.is_empty(),
            "{peer}: both are private: {:?}",
            apart.keys()
        );
    }
}
