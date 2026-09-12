//! `PairingSession` end to end: happy path, one-sided confirmation, rejection closing the window,
//! and the MITM test — the acceptance test for the whole task (task notes).

use txtodo_model::{DeviceId, Ulid};

use crate::message::GroupId;
use crate::nonce_registry::{MAX_CONCURRENT_PAIRINGS, NonceRegistry};
use crate::pairing::{MAX_FAILED_SAS_CONFIRMATIONS, PairingSession};
use crate::pairing_error::PairingError;

fn device(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

/// Runs a full honest handshake between two devices, returning both completed sessions.
fn handshake(
    device_a: DeviceId,
    device_b: DeviceId,
    group: GroupId,
) -> (PairingSession, PairingSession) {
    let mut reg_a = NonceRegistry::new();
    let mut reg_b = NonceRegistry::new();
    let (mut session_a, offer) =
        PairingSession::offer(device_a, group, "10.0.0.1:4242".to_string(), 0, &mut reg_a).unwrap();
    let (session_b, pub_b) = PairingSession::accept(device_b, &offer, 0, &mut reg_b).unwrap();
    session_a.complete(device_b, pub_b, 0, &mut reg_a).unwrap();
    (session_a, session_b)
}

#[test]
fn happy_path_yields_identical_sas_on_both_sides() {
    let (a, b) = handshake(device(1), device(2), GroupId(1));
    assert_eq!(a.sas_words().unwrap(), b.sas_words().unwrap());
    assert_eq!(a.peer_device(), Some(device(2)));
    assert_eq!(b.peer_device(), Some(device(1)));
}

#[test]
fn group_key_moves_only_after_both_sides_confirm() {
    let (mut a, mut b) = handshake(device(1), device(2), GroupId(1));
    let group_key = b"the actual group key material..";

    // Neither confirmed yet.
    assert_eq!(a.wrap_group_key(group_key), Err(PairingError::NotConfirmed));

    a.confirm_local().unwrap();
    // One-sided: A confirmed locally, but A's remote flag (B's confirmation reaching A) has not
    // arrived. Must still refuse — a one-sided confirm transfers nothing (task notes).
    assert_eq!(a.wrap_group_key(group_key), Err(PairingError::NotConfirmed));

    b.confirm_local().unwrap();
    a.confirm_remote().unwrap(); // B's confirmation reaches A
    b.confirm_remote().unwrap(); // A's confirmation reaches B

    assert!(a.is_ready_to_send_key());
    assert!(b.is_ready_to_send_key());
    let sealed = a.wrap_group_key(group_key).unwrap();
    assert_eq!(b.unwrap_group_key(&sealed).unwrap(), group_key);
}

/// `sync-device-remove`'s own notes: the static key has to be registered *at pairing*, or
/// rotation has nothing to wrap a future group key to. This is that registration: each side's
/// static public key rides inside the same confirmed exchange as the group key itself.
#[test]
fn pairing_also_registers_each_sides_static_public_key() {
    use crate::device_static::DeviceStaticSecret;
    use crate::pairing_grant::PairingGrant;

    let (mut a, mut b) = handshake(device(1), device(2), GroupId(1));
    a.confirm_local().unwrap();
    b.confirm_local().unwrap();
    a.confirm_remote().unwrap();
    b.confirm_remote().unwrap();

    let a_static = DeviceStaticSecret::generate();
    let b_static = DeviceStaticSecret::generate();
    let group_key = [5u8; 32];

    let from_a = PairingGrant {
        group_key,
        static_public: a_static.public_key().to_bytes(),
    };
    let sealed = a.wrap_grant(&from_a).unwrap();
    let seen_by_b = b.unwrap_grant(&sealed).unwrap();
    assert_eq!(seen_by_b.group_key, group_key);
    assert_eq!(seen_by_b.static_public, a_static.public_key().to_bytes());

    // The exchange is symmetric: B registers its own static key back to A the same way.
    let from_b = PairingGrant {
        group_key,
        static_public: b_static.public_key().to_bytes(),
    };
    let sealed_back = b.wrap_grant(&from_b).unwrap();
    let seen_by_a = a.unwrap_grant(&sealed_back).unwrap();
    assert_eq!(seen_by_a.static_public, b_static.public_key().to_bytes());
}

#[test]
fn one_sided_confirmation_transfers_no_key_on_either_side() {
    let (mut a, b) = handshake(device(1), device(2), GroupId(1));
    a.confirm_local().unwrap();
    // B never confirms; A never learns of a remote confirmation either.
    assert_eq!(a.wrap_group_key(b"key"), Err(PairingError::NotConfirmed));
    assert_eq!(b.wrap_group_key(b"key"), Err(PairingError::NotConfirmed));
}

#[test]
fn repeated_rejection_closes_the_window_and_stops_accepting_confirmations() {
    let (mut a, _b) = handshake(device(1), device(2), GroupId(1));
    for _ in 0..MAX_FAILED_SAS_CONFIRMATIONS - 1 {
        a.reject().unwrap();
    }
    a.reject().unwrap(); // reaches the cap, closes
    assert_eq!(a.confirm_local(), Err(PairingError::Closed));
    assert_eq!(a.reject(), Err(PairingError::Closed));
    assert_eq!(a.wrap_group_key(b"key"), Err(PairingError::Closed));
}

#[test]
fn an_expired_nonce_fails_accept_and_leaves_no_session() {
    let mut reg_a = NonceRegistry::new();
    let (_session_a, offer) = PairingSession::offer(
        device(1),
        GroupId(1),
        "10.0.0.1:4242".to_string(),
        0,
        &mut reg_a,
    )
    .unwrap();
    let mut reg_b = NonceRegistry::new();
    let err = PairingSession::accept(
        device(2),
        &offer,
        crate::nonce_registry::PAIRING_WINDOW_MS + 1,
        &mut reg_b,
    )
    .unwrap_err();
    assert_eq!(
        err,
        PairingError::Nonce(crate::nonce_registry::NonceError::Expired)
    );
}

#[test]
fn a_reused_offer_is_refused_on_the_second_accept() {
    let mut reg_a = NonceRegistry::new();
    let (_session_a, offer) = PairingSession::offer(
        device(1),
        GroupId(1),
        "10.0.0.1:4242".to_string(),
        0,
        &mut reg_a,
    )
    .unwrap();
    let mut reg_b = NonceRegistry::new();
    PairingSession::accept(device(2), &offer, 0, &mut reg_b).unwrap();
    let err = PairingSession::accept(device(3), &offer, 0, &mut reg_b).unwrap_err();
    assert_eq!(
        err,
        PairingError::Nonce(crate::nonce_registry::NonceError::AlreadyConsumed)
    );
}

#[test]
fn max_concurrent_pairings_constant_is_one() {
    assert_eq!(MAX_CONCURRENT_PAIRINGS, 1);
}

/// The acceptance test for the whole task: a relay running two independent handshakes — one with
/// the real initiator, one with the real joiner, substituting its own ephemeral key and identity on
/// each leg — must not be able to make the two honest sides land on the same SAS. If it could, the
/// six words would not actually catch a machine-in-the-middle.
#[test]
fn mitm_relay_running_two_handshakes_produces_two_different_sas() {
    let device_a = device(1); // real initiator
    let device_b = device(2); // real joiner
    let device_m = device(3); // attacker, on the leg facing A
    let group = GroupId(1);

    // Leg 1: A believes it is pairing with the peer at the other end of `offer`; the attacker
    // accepts it directly (playing the joiner) using its own device id and ephemeral key.
    let mut reg_a = NonceRegistry::new();
    let (mut session_a, offer) =
        PairingSession::offer(device_a, group, "10.0.0.1:4242".to_string(), 0, &mut reg_a).unwrap();
    let mut reg_m1 = NonceRegistry::new();
    let (_session_m1, pub_m1) = PairingSession::accept(device_m, &offer, 0, &mut reg_m1).unwrap();
    session_a.complete(device_m, pub_m1, 0, &mut reg_a).unwrap();

    // Leg 2: the attacker crafts a second offer toward B, claiming A's device id (spoofed) but
    // using a *second*, independent ephemeral key it generated for this leg.
    let mut reg_m2 = NonceRegistry::new();
    let (_session_m2, fake_offer) =
        PairingSession::offer(device_a, group, "10.0.0.2:4242".to_string(), 0, &mut reg_m2)
            .unwrap();
    let mut reg_b = NonceRegistry::new();
    let (session_b, _pub_b) = PairingSession::accept(device_b, &fake_offer, 0, &mut reg_b).unwrap();

    let sas_a = session_a.sas_words().unwrap();
    let sas_b = session_b.sas_words().unwrap();
    assert_ne!(
        sas_a, sas_b,
        "a relay running two independent handshakes must not produce matching SAS on both ends"
    );
}
