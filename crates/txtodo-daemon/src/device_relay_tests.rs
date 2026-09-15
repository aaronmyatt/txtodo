//! `WorkspaceRoutes`: pure data-structure tests, no networking — `DeviceRelay::bind`'s own real
//! endpoint bind is exercised indirectly by every real-daemon relay test in `tests/` instead
//! (mirrors `workspace_offer_registry_tests.rs`'s own split between table logic and transport).

use crate::device_relay::{
    DeviceRelayError, MAX_ROUTED_WORKSPACES, WorkspaceRoute, WorkspaceRoutes,
};
use crate::lan_session_tests::make_workspace;
use std::sync::Arc;
use txtodo_model::Ulid;
use txtodo_store::WorkspaceId;

/// A route pointing at a fresh, real (if throwaway) workspace — `WorkspaceRoutes` never inspects
/// `ws`/`device`/`group` itself, so any real `SharedWorkspace` proves the table works the same way
/// `lan_session_tests.rs`'s own fixtures already do for this crate's other tables.
fn one_route(dir: &std::path::Path) -> WorkspaceRoute {
    let (ws, device, group, _workspace) = make_workspace(dir, [1u8; 32]);
    WorkspaceRoute { ws, device, group }
}

#[test]
fn register_then_route_returns_the_exact_route_unregister_then_route_does_not() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let routes = WorkspaceRoutes::new();
    let id = WorkspaceId::new(Ulid::from_u128(1));
    let route = one_route(dir.path());
    let (device, group) = (route.device, route.group);
    assert!(routes.route(id).is_none());
    routes.register(id, route).unwrap_or_else(|e| panic!("{e}"));
    let got = routes
        .route(id)
        .unwrap_or_else(|| panic!("route must be present"));
    assert_eq!(
        got.device, device,
        "the exact device this route was registered with"
    );
    assert_eq!(
        got.group, group,
        "the exact group this route was registered with"
    );
    let same_ws = Arc::ptr_eq(&got.ws, &got.ws.clone());
    assert!(same_ws, "sanity: SharedWorkspace clones share the same Arc");
    routes.unregister(id);
    assert!(routes.route(id).is_none());
}

#[test]
fn an_unknown_id_is_none_not_a_panic() {
    let routes = WorkspaceRoutes::new();
    let unknown = WorkspaceId::new(Ulid::from_u128(999));
    assert!(routes.route(unknown).is_none());
}

#[test]
fn re_registering_the_same_id_never_counts_against_the_cap() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let routes = WorkspaceRoutes::new();
    let id = WorkspaceId::new(Ulid::from_u128(1));
    for _ in 0..3 {
        routes
            .register(id, one_route(dir.path()))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(routes.route(id).is_some());
}

#[test]
fn a_genuinely_new_route_past_the_cap_is_refused() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let routes = WorkspaceRoutes::new();
    for n in 0..MAX_ROUTED_WORKSPACES {
        let id = WorkspaceId::new(Ulid::from_u128(n as u128));
        routes
            .register(id, one_route(dir.path()))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let one_more = WorkspaceId::new(Ulid::from_u128(MAX_ROUTED_WORKSPACES as u128));
    assert!(matches!(
        routes.register(one_more, one_route(dir.path())),
        Err(DeviceRelayError::TooManyRoutes)
    ));
}
