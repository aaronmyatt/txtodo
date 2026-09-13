//! This daemon's own pairing bookkeeping: at most one active [`PairingSession`]
//! (`MAX_CONCURRENT_PAIRINGS`), its own `NonceRegistry`, and `PAIRING_WINDOW_MS` expiry —
//! everything `pairing_grpc.rs` needs to drive `txtodo-sync`'s state machine from gRPC.
//!
//! The three original gRPC RPCs only ever reached this device's *own* daemon; the leg between two
//! daemons (the joiner's public key reaching the initiator, and the sealed group key reaching the
//! joiner back) now has a real transport: `pairing_lan.rs` drives the relay-seam methods below
//! (`complete_as_initiator`, `joiner_public_key`, `mark_remote_confirmed`, `try_finalize_initiator`,
//! `adopt_group_key`, plus the read-only [`PairingRegistry::snapshot`] and the peer-static-key
//! bookkeeping) over the LAN `Link` `sync-lan-transport` built, on its own ALPN
//! (`txtodo_sync::PAIRING_ALPN`) so a pairing connection is never confused with the group-keyed
//! sync protocol. `pairing_grpc_tests.rs` still drives every method directly too (whitebox,
//! same-process) — exactly how a real two-daemon test drives the same methods through a real
//! socket instead.

use std::sync::Mutex;

use txtodo_model::DeviceId;
use txtodo_sync::{
    DEVICE_STATIC_KEY_BYTES, DeviceStaticPublic, GroupId, KEY_BYTES, KeyId, KeyStore,
    MAX_CONCURRENT_PAIRINGS, Nonce, NonceRegistry, PAIRING_WINDOW_MS, PairingError, PairingGrant,
    PairingOffer, PairingSession, SAS_WORD_COUNT, Secret, X25519_PUBLIC_KEY_BYTES,
};

pub(crate) use crate::pairing_state_error::PairingStateError;

/// The group-key epoch pairing establishes. Rotation (`txtodo device remove`) is future work.
const INITIAL_GROUP_EPOCH: u32 = 0;

/// Which role this daemon is playing in its one active pairing attempt.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// This daemon called `pair_offer` and is waiting for a peer to accept.
    Initiator,
    /// This daemon called `pair_accept` against a peer's offer.
    Joiner,
}

struct Active {
    session: PairingSession,
    role: Role,
    opened_at_ms: u64,
    /// The joiner's own ephemeral public key, kept so a relay can hand it to the initiator's
    /// `complete_as_initiator`. Only ever set on the joiner's side.
    own_public: Option<[u8; X25519_PUBLIC_KEY_BYTES]>,
    /// The peer's long-term static public key, learned from the network (`pairing_lan.rs`'s
    /// `JoinerHello`) before both sides have confirmed. Only ever set on the initiator's side —
    /// the joiner learns the initiator's static key later, bundled inside the sealed
    /// [`PairingGrant`] itself, so it has no need to stash one here.
    peer_static: Option<[u8; DEVICE_STATIC_KEY_BYTES]>,
}

/// A read-only snapshot of the active pairing, enough for the LAN relay driver (`pairing_lan.rs`)
/// to validate an incoming `JoinerHello` before touching any crypto state.
pub(crate) struct ActiveSnapshot {
    pub(crate) role: Role,
    pub(crate) group: GroupId,
    pub(crate) nonce: Nonce,
    pub(crate) is_handshaken: bool,
    pub(crate) peer_device: Option<DeviceId>,
}

#[derive(Default)]
struct Inner {
    nonces: NonceRegistry,
    active: Option<Active>,
}

/// Clears `active` and reports why it (or the lack of it) refuses the caller's next step:
/// [`PairingStateError::NotActive`] when nothing has started, [`PairingStateError::WindowExpired`]
/// (clearing it) when it started but the window elapsed, else a live borrow of it.
fn active_mut(active: &mut Option<Active>, now_ms: u64) -> Result<&mut Active, PairingStateError> {
    let expired = match active.as_ref() {
        None => return Err(PairingStateError::NotActive),
        Some(a) => now_ms.saturating_sub(a.opened_at_ms) > PAIRING_WINDOW_MS,
    };
    if expired {
        *active = None;
        return Err(PairingStateError::WindowExpired);
    }
    active.as_mut().ok_or(PairingStateError::NotActive)
}

/// Refuses a new attempt at `MAX_CONCURRENT_PAIRINGS` (an honest, still-live one already exists);
/// silently lets go of an expired one first, since expiry must not jam the one slot forever.
fn ensure_capacity(active: &mut Option<Active>, now_ms: u64) -> Result<(), PairingStateError> {
    debug_assert_eq!(
        MAX_CONCURRENT_PAIRINGS, 1,
        "one slot models the cap directly"
    );
    let Some(a) = active.as_ref() else {
        return Ok(());
    };
    if now_ms.saturating_sub(a.opened_at_ms) > PAIRING_WINDOW_MS {
        *active = None;
        Ok(())
    } else {
        Err(PairingStateError::TooManyOpen)
    }
}

/// Fetches this workspace's current group key bytes, minting and storing a fresh one if none has
/// ever been created (the first-ever pairing for a brand-new group). `getrandom` failure is not
/// recoverable in a meaningful way (`workspace.rs::load_or_mint_group` takes the same stance).
/// Only called by [`PairingRegistry::try_finalize_initiator`] (see its own doc on why that is
/// itself only exercised by `pairing_grpc_tests.rs` today).
#[allow(dead_code)]
fn fetch_or_mint_group_key(key_store: &dyn KeyStore) -> Result<[u8; KEY_BYTES], PairingStateError> {
    if let Some(secret) = key_store.get(KeyId::Group(INITIAL_GROUP_EPOCH))? {
        let bytes: [u8; KEY_BYTES] = secret
            .expose()
            .try_into()
            .map_err(|_| PairingStateError::CorruptGroupKey(secret.expose().len()))?;
        return Ok(bytes);
    }
    let mut bytes = [0u8; KEY_BYTES];
    if getrandom::fill(&mut bytes).is_err() {
        bytes = [0xA5; KEY_BYTES];
    }
    key_store.put(
        KeyId::Group(INITIAL_GROUP_EPOCH),
        &Secret::new(bytes.to_vec()),
    )?;
    Ok(bytes)
}

/// This daemon's pairing bookkeeping: at most [`MAX_CONCURRENT_PAIRINGS`] session, its own nonces.
#[derive(Default)]
pub(crate) struct PairingRegistry {
    inner: Mutex<Inner>,
}

impl PairingRegistry {
    /// An empty registry.
    pub(crate) fn new() -> PairingRegistry {
        PairingRegistry::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts a pairing as the initiator (`pair_offer`).
    pub(crate) fn begin_offer(
        &self,
        own_device: DeviceId,
        group: GroupId,
        endpoint: String,
        now_ms: u64,
    ) -> Result<PairingOffer, PairingStateError> {
        let mut inner = self.lock();
        ensure_capacity(&mut inner.active, now_ms)?;
        let (session, offer) =
            PairingSession::offer(own_device, group, endpoint, now_ms, &mut inner.nonces)?;
        inner.active = Some(Active {
            session,
            role: Role::Initiator,
            opened_at_ms: now_ms,
            own_public: None,
            peer_static: None,
        });
        Ok(offer)
    }

    /// Accepts a peer's offer as the joiner (`pair_accept`); the SAS is available immediately.
    pub(crate) fn begin_accept(
        &self,
        own_device: DeviceId,
        offer: &PairingOffer,
        now_ms: u64,
    ) -> Result<[&'static str; SAS_WORD_COUNT], PairingStateError> {
        let mut inner = self.lock();
        ensure_capacity(&mut inner.active, now_ms)?;
        let (session, own_public) =
            PairingSession::accept(own_device, offer, now_ms, &mut inner.nonces)?;
        let sas = session.sas_words()?;
        inner.active = Some(Active {
            session,
            role: Role::Joiner,
            opened_at_ms: now_ms,
            own_public: Some(own_public),
            peer_static: None,
        });
        Ok(sas)
    }

    /// Records this device's human confirming the SAS (`pair_confirm_sas`).
    pub(crate) fn confirm_local(
        &self,
        now_ms: u64,
    ) -> Result<[&'static str; SAS_WORD_COUNT], PairingStateError> {
        let mut inner = self.lock();
        let active = active_mut(&mut inner.active, now_ms)?;
        active.session.confirm_local()?;
        Ok(active.session.sas_words()?)
    }

    /// Relay seam: the joiner's own public key, for handing to the initiator's
    /// [`PairingRegistry::complete_as_initiator`]. Driven for real by `pairing_lan.rs`'s incoming
    /// `JoinerHello` handler; `pairing_grpc_tests.rs` also drives it directly (whitebox).
    pub(crate) fn joiner_public_key(
        &self,
        now_ms: u64,
    ) -> Result<[u8; X25519_PUBLIC_KEY_BYTES], PairingStateError> {
        let mut inner = self.lock();
        let active = active_mut(&mut inner.active, now_ms)?;
        if active.role != Role::Joiner {
            return Err(PairingStateError::WrongRole);
        }
        active.own_public.ok_or(PairingStateError::NotActive)
    }

    /// Relay seam: completes the initiator's side of the handshake once the joiner's public key
    /// arrives, so this device can compute the same transcript/SAS/shared secret. Driven for real
    /// by `pairing_lan.rs`'s incoming `JoinerHello` handler (after it has checked the hello's
    /// nonce/group against [`PairingRegistry::snapshot`] itself); `pairing_grpc_tests.rs` also
    /// drives it directly (whitebox).
    pub(crate) fn complete_as_initiator(
        &self,
        peer_device: DeviceId,
        peer_public: [u8; X25519_PUBLIC_KEY_BYTES],
        now_ms: u64,
    ) -> Result<(), PairingStateError> {
        let mut inner = self.lock();
        let Inner { active, nonces } = &mut *inner;
        let entry = active_mut(active, now_ms)?;
        if entry.role != Role::Initiator {
            return Err(PairingStateError::WrongRole);
        }
        entry
            .session
            .complete(peer_device, peer_public, now_ms, nonces)?;
        Ok(())
    }

    /// Relay seam: records that the peer's SAS confirmation arrived. Driven for real by
    /// `pairing_lan.rs` when a `JoinerHello.confirmed` arrives true; `pairing_grpc_tests.rs` also
    /// drives it directly (whitebox).
    pub(crate) fn mark_remote_confirmed(&self, now_ms: u64) -> Result<(), PairingStateError> {
        let mut inner = self.lock();
        let active = active_mut(&mut inner.active, now_ms)?;
        active.session.confirm_remote()?;
        Ok(())
    }

    /// Relay seam: once both sides have confirmed, the initiator wraps its group key (minting one
    /// first if this is a brand-new group) **and** `own_static_public` — its own long-term X25519
    /// static key (plan M4 `sync-device-remove`), bundled via [`PairingGrant`]/`wrap_grant` rather
    /// than the bare `wrap_group_key`, so the joiner learns a static key it can be handed a
    /// rotation grant to later, registered nowhere before this call. Returns `None` when not yet
    /// ready, or when this daemon is not the initiator — the joiner has nothing to send. Clears
    /// `active` on success, so `pairing_lan.rs`'s driver caches the returned sealed bytes itself
    /// (network-layer retry concern, not this registry's) rather than calling this a second time.
    /// Driven for real by `pairing_lan.rs`; `pairing_grpc_tests.rs` also drives it directly
    /// (whitebox).
    pub(crate) fn try_finalize_initiator(
        &self,
        key_store: &dyn KeyStore,
        own_static_public: DeviceStaticPublic,
        now_ms: u64,
    ) -> Result<Option<Vec<u8>>, PairingStateError> {
        let mut inner = self.lock();
        let ready = {
            let active = active_mut(&mut inner.active, now_ms)?;
            active.role == Role::Initiator && active.session.is_ready_to_send_key()
        };
        if !ready {
            return Ok(None);
        }
        let group_key = fetch_or_mint_group_key(key_store)?;
        let grant = PairingGrant {
            group_key,
            static_public: own_static_public.to_bytes(),
        };
        let sealed = {
            let active = active_mut(&mut inner.active, now_ms)?;
            active.session.wrap_grant(&grant)?
        };
        inner.active = None;
        Ok(Some(sealed))
    }

    /// Relay seam: the joiner's side of the same finish — unwraps the initiator's
    /// [`PairingGrant`], stores the group key, and this side's pairing finishes too. Returns the
    /// initiator's `DeviceId` and long-term static public key so the caller
    /// ([`crate::workspace::Workspace::adopt_group_key`]) can register it in the `devices` table.
    /// The reverse direction (the initiator learning the joiner's static key) is
    /// [`PairingRegistry::peer_static`] below, populated from the network by `pairing_lan.rs`'s
    /// incoming `JoinerHello` handler rather than bundled in a grant, since the joiner sends its
    /// static key before either side has confirmed (see `pairing_relay.rs`'s module doc on why
    /// that is safe: it is no more secret than the ephemeral key exchanged the same way). Driven
    /// for real by `pairing_lan.rs`; `pairing_grpc_tests.rs` also drives it directly (whitebox).
    pub(crate) fn adopt_group_key(
        &self,
        key_store: &dyn KeyStore,
        sealed: &[u8],
        now_ms: u64,
    ) -> Result<(DeviceId, DeviceStaticPublic), PairingStateError> {
        let mut inner = self.lock();
        let (group_key, peer_device, peer_static) = {
            let active = active_mut(&mut inner.active, now_ms)?;
            if active.role != Role::Joiner {
                return Err(PairingStateError::WrongRole);
            }
            if !active.session.is_ready_to_send_key() {
                return Err(PairingError::NotConfirmed.into());
            }
            let grant = active.session.unwrap_grant(sealed)?;
            let peer_device = active
                .session
                .peer_device()
                .ok_or(PairingStateError::NotActive)?;
            (
                grant.group_key,
                peer_device,
                DeviceStaticPublic::from_bytes(grant.static_public),
            )
        };
        key_store.put(
            KeyId::Group(INITIAL_GROUP_EPOCH),
            &Secret::new(group_key.to_vec()),
        )?;
        inner.active = None;
        Ok((peer_device, peer_static))
    }

    /// A read-only [`ActiveSnapshot`] of whatever pairing is active, for `pairing_lan.rs` to
    /// validate an incoming `JoinerHello`'s `group`/`nonce` before calling any method above that
    /// would mutate state (in particular, before `complete_as_initiator`, which can only ever
    /// succeed once per session).
    pub(crate) fn snapshot(&self, now_ms: u64) -> Result<ActiveSnapshot, PairingStateError> {
        let mut inner = self.lock();
        let active = active_mut(&mut inner.active, now_ms)?;
        Ok(ActiveSnapshot {
            role: active.role,
            group: active.session.group(),
            nonce: active.session.nonce(),
            is_handshaken: active.session.is_handshaken(),
            peer_device: active.session.peer_device(),
        })
    }

    /// Records the joiner's long-term static public key, learned from its `JoinerHello` — the
    /// reverse leg of the static-key exchange `adopt_group_key`'s doc names (the initiator learning
    /// the joiner's key, rather than the other way around). Only meaningful on the initiator's
    /// side; overwrites silently on a retried `JoinerHello`, which always resends the same key.
    pub(crate) fn set_peer_static(
        &self,
        static_public: [u8; DEVICE_STATIC_KEY_BYTES],
        now_ms: u64,
    ) -> Result<(), PairingStateError> {
        let mut inner = self.lock();
        let active = active_mut(&mut inner.active, now_ms)?;
        active.peer_static = Some(static_public);
        Ok(())
    }

    /// The joiner's static public key recorded by [`PairingRegistry::set_peer_static`], if any —
    /// read once both sides have confirmed, to register the joiner symmetrically in the
    /// initiator's own `devices` table.
    pub(crate) fn peer_static(&self, now_ms: u64) -> Option<[u8; DEVICE_STATIC_KEY_BYTES]> {
        let mut inner = self.lock();
        active_mut(&mut inner.active, now_ms).ok()?.peer_static
    }

    /// Whether *this* device's own human has confirmed the SAS yet — read by the joiner's
    /// background relay task (`pairing_lan.rs`) on every retry so a `JoinerHello.confirmed` always
    /// reflects the current, real state rather than a value captured once at the start of pairing.
    pub(crate) fn joiner_local_confirmed(&self, now_ms: u64) -> Result<bool, PairingStateError> {
        let mut inner = self.lock();
        let active = active_mut(&mut inner.active, now_ms)?;
        Ok(active.session.is_locally_confirmed())
    }
}
