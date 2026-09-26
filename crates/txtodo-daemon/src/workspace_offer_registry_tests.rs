//! `WorkspaceOfferRegistry` (task `daemon-workspace-identity-agreement` stage 4): multi-offer
//! coexistence (unlike `PairingRegistry`'s single slot), idempotent re-announce, take-consumes,
//! and the `MAX_PENDING_OFFERS` cap.

use txtodo_model::{DeviceId, Ulid};
use txtodo_store::WorkspaceId;

use crate::workspace_offer_registry::{
    MAX_PENDING_OFFERS, PendingOffer, WorkspaceOfferRegistry, WorkspaceOfferRegistryError,
};

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn workspace(n: u128) -> WorkspaceId {
    WorkspaceId::new(Ulid::from_u128(n))
}

fn offer(device_n: u128, workspace_n: u128, name: &str) -> PendingOffer {
    PendingOffer {
        offering_device: device(device_n),
        workspace_id: workspace(workspace_n),
        name: name.to_string(),
        offered_at_ms: 1_000,
    }
}

#[test]
fn record_list_and_take_round_trip() {
    let registry = WorkspaceOfferRegistry::new();
    registry.record(offer(1, 1, "alpha")).unwrap();

    let listed = registry.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "alpha");

    let taken = registry.take(device(1), workspace(1));
    assert_eq!(taken, Some(offer(1, 1, "alpha")));
    assert!(registry.list().is_empty(), "take removes it");
    assert_eq!(
        registry.take(device(1), workspace(1)),
        None,
        "already consumed"
    );
}

#[test]
fn many_pending_offers_coexist_unlike_pairings_single_slot() {
    let registry = WorkspaceOfferRegistry::new();
    registry.record(offer(1, 1, "from device 1")).unwrap();
    registry.record(offer(2, 1, "from device 2")).unwrap();
    registry
        .record(offer(1, 2, "a second workspace from device 1"))
        .unwrap();

    assert_eq!(registry.list().len(), 3, "all three coexist");
}

#[test]
fn re_announcing_the_same_offer_replaces_it_in_place_not_a_duplicate() {
    let registry = WorkspaceOfferRegistry::new();
    registry.record(offer(1, 1, "alpha")).unwrap();
    registry
        .record(PendingOffer {
            offered_at_ms: 2_000,
            ..offer(1, 1, "alpha (renamed)")
        })
        .unwrap();

    let listed = registry.list();
    assert_eq!(listed.len(), 1, "still one entry, not two");
    assert_eq!(listed[0].name, "alpha (renamed)");
    assert_eq!(listed[0].offered_at_ms, 2_000);
}

#[test]
fn a_genuinely_new_offer_past_the_cap_is_refused() {
    let registry = WorkspaceOfferRegistry::new();
    for n in 0..MAX_PENDING_OFFERS as u128 {
        registry.record(offer(n, n, "filler")).unwrap();
    }
    let err = registry
        .record(offer(
            MAX_PENDING_OFFERS as u128,
            MAX_PENDING_OFFERS as u128,
            "one too many",
        ))
        .expect_err("the cap is full");
    assert!(matches!(err, WorkspaceOfferRegistryError::TooManyPending));
    assert_eq!(registry.list().len(), MAX_PENDING_OFFERS);
}

#[test]
fn re_announcing_an_existing_offer_never_counts_against_the_cap() {
    let registry = WorkspaceOfferRegistry::new();
    for n in 0..MAX_PENDING_OFFERS as u128 {
        registry.record(offer(n, n, "filler")).unwrap();
    }
    // A re-announce of an already-pending pair must succeed even at capacity.
    registry
        .record(offer(0, 0, "filler, re-announced"))
        .unwrap();
    assert_eq!(registry.list().len(), MAX_PENDING_OFFERS);
}

/// Task sync-drift line 8: an offer is remembered as seen after the mirror task took it, a
/// declined pair still counts as offered, and the newest device comes first.
#[test]
fn offered_by_outlives_take_counts_declined_and_puts_the_newest_first() {
    let registry = WorkspaceOfferRegistry::new();
    let hour = std::time::Duration::from_secs(3_600);
    assert!(registry.offered_by(workspace(1), hour).is_empty());

    registry.record(offer(1, 1, "alpha")).unwrap();
    registry.take(device(1), workspace(1));
    std::thread::sleep(std::time::Duration::from_millis(2));
    registry.record(offer(2, 1, "alpha")).unwrap();
    assert!(registry.decline(device(2), workspace(1)));
    registry.record(offer(2, 1, "alpha")).unwrap();
    registry.record(offer(3, 9, "other")).unwrap();

    assert_eq!(
        registry.offered_by(workspace(1), hour),
        [device(2), device(1)]
    );
    assert!(
        registry
            .list()
            .iter()
            .all(|o| o.workspace_id != workspace(1))
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(
        registry
            .offered_by(workspace(1), std::time::Duration::from_millis(1))
            .is_empty(),
        "too long ago"
    );
}
