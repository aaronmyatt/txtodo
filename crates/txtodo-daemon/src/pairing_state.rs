//! This daemon's own pairing bookkeeping: at most one active [`PairingSession`]
//! (`MAX_CONCURRENT_PAIRINGS`), its own `NonceRegistry`, and `PAIRING_WINDOW_MS` expiry —
//! everything `pairing_grpc.rs` needs to drive `txtodo-sync`'s state machine from gRPC.
//!
//! The three gRPC RPCs only reach this device's *own* daemon; the leg between two daemons (the
//! joiner's public key reaching the initiator, and the sealed group key reaching the joiner back)
//! has no transport yet (`sync-lan-transport` is separate, later work — see `pairing_grpc.rs`'s
//! module doc). The `_relay`-suited methods below (`complete_as_initiator`, `joiner_public_key`,
//! `mark_remote_confirmed`, `try_finalize_initiator`, `adopt_group_key`) are that leg's seam: not
//! reachable from any of the three RPCs, driven directly by `pairing_grpc_tests.rs` today, the way
//! a real transport will drive them once it exists.

use std::sync::Mutex;

use txtodo_model::DeviceId;
use txtodo_sync::{
    DeviceStaticPublic, GroupId, KEY_BYTES, KeyId, KeyStore, KeyStoreError,
    MAX_CONCURRENT_PAIRINGS, NonceRegistry, PAIRING_WINDOW_MS, PairingError, PairingGrant,
    PairingOffer, PairingSession, SAS_WORD_COUNT, Secret, X25519_PUBLIC_KEY_BYTES,
};

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
    /// `complete_as_initiator`. Only ever set on the joiner's side; only read by the relay-seam
    /// methods below, which today only `pairing_grpc_tests.rs` calls (see module doc) — hence the
    /// `allow` rather than a live, non-test caller.
    #[allow(dead_code)]
    own_public: Option<[u8; X25519_PUBLIC_KEY_BYTES]>,
}

/// Why a pairing step was refused: this daemon's own bookkeeping folded in with
/// [`PairingError`] so `pairing_grpc.rs` has one error type to map to a `Status`.
#[derive(Debug)]
pub(crate) enum PairingStateError {
    /// `MAX_CONCURRENT_PAIRINGS` already active and not expired.
    TooManyOpen,
    /// The active pairing's `PAIRING_WINDOW_MS` window elapsed; it has been cleared.
    WindowExpired,
    /// No pairing is active on this daemon.
    NotActive,
    /// The active pairing exists but is not in the role this call needs.
    WrongRole,
    /// The state machine itself refused the step.
    Session(PairingError),
    /// The keystore refused a read or write.
    KeyStore(KeyStoreError),
    /// Persisting the adopted group id to the store's `meta` table failed.
    Store(txtodo_store::StoreError),
    /// The stored group key is not `KEY_BYTES` long (the keystore was edited or corrupted by hand).
    CorruptGroupKey(usize),
}

impl std::fmt::Display for PairingStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingStateError::TooManyOpen => {
                write!(
                    f,
                    "{MAX_CONCURRENT_PAIRINGS} pairing(s) already open on this daemon"
                )
            }
            PairingStateError::WindowExpired => {
                write!(f, "the pairing window ({PAIRING_WINDOW_MS} ms) has expired")
            }
            PairingStateError::NotActive => write!(f, "no pairing is active on this daemon"),
            PairingStateError::WrongRole => {
                write!(f, "the active pairing is not in the role this call needs")
            }
            PairingStateError::Session(e) => write!(f, "{e}"),
            PairingStateError::KeyStore(e) => write!(f, "{e}"),
            PairingStateError::Store(e) => write!(f, "{e}"),
            PairingStateError::CorruptGroupKey(len) => {
                write!(f, "stored group key is {len} bytes, not {KEY_BYTES}")
            }
        }
    }
}

impl std::error::Error for PairingStateError {}

impl From<PairingError> for PairingStateError {
    fn from(e: PairingError) -> PairingStateError {
        PairingStateError::Session(e)
    }
}

impl From<KeyStoreError> for PairingStateError {
    fn from(e: KeyStoreError) -> PairingStateError {
        PairingStateError::KeyStore(e)
    }
}

impl From<txtodo_store::StoreError> for PairingStateError {
    fn from(e: txtodo_store::StoreError) -> PairingStateError {
        PairingStateError::Store(e)
    }
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
    /// [`PairingRegistry::complete_as_initiator`]. See the module doc — no transport calls this
    /// yet, only `pairing_grpc_tests.rs`.
    #[allow(dead_code)]
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
    /// arrives, so this device can compute the same transcript/SAS/shared secret. See the module
    /// doc — no transport calls this yet, only `pairing_grpc_tests.rs`.
    #[allow(dead_code)]
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

    /// Relay seam: records that the peer's SAS confirmation arrived. See the module doc — no
    /// transport calls this yet, only `pairing_grpc_tests.rs`.
    #[allow(dead_code)]
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
    /// ready, or when this daemon is not the initiator — the joiner has nothing to send. See the
    /// module doc — no transport calls this yet, only `pairing_grpc_tests.rs`.
    #[allow(dead_code)]
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
    /// ([`crate::workspace::Workspace::adopt_group_key`]) can register it in the `devices` table —
    /// this is the only leg of the static-key exchange this daemon wires today; the reverse
    /// direction (the initiator learning the joiner's static key) needs a real transport to carry
    /// a second grant back, which does not exist yet (`sync-lan-transport`, separate work). See the
    /// module doc — no transport calls this yet, only `pairing_grpc_tests.rs`.
    #[allow(dead_code)]
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
}
