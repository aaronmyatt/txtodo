//! The pairing handshake state machine: X25519 ephemeral keys in, a confirmed group-key transfer
//! out. Transport-agnostic like the rest of this crate — nothing here opens a socket or shows a QR;
//! the caller moves the [`crate::offer::PairingOffer`] bytes and the confirmation signals however
//! it likes (direct function calls today, same as the tests).
//!
//! Two devices reach the same [`crate::sas::sas_words`] only if they agree on the same
//! [`crate::transcript::transcript`], which is why an active machine-in-the-middle running two
//! independent handshakes cannot make both victims read the same six words — see the MITM test.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::OsRng;
use txtodo_model::DeviceId;
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::frame::PROTOCOL_VERSION;
use crate::message::GroupId;
use crate::nonce_registry::{Nonce, NonceRegistry};
use crate::offer::PairingOffer;
use crate::pairing_error::PairingError;
use crate::pairing_grant::PairingGrant;
use crate::sas::{PAIR_KEY_BYTES, SAS_WORD_COUNT, pair_key, sas_words};
use crate::transcript::{Party, TRANSCRIPT_BYTES, X25519_PUBLIC_KEY_BYTES, transcript};

/// Mismatched SAS confirmations tolerated before the window closes outright ("rate-limit, then
/// close" rather than close-on-first-mismatch, since a human can fat-finger confirm/reject).
pub const MAX_FAILED_SAS_CONFIRMATIONS: u32 = 3;

/// Bytes prepended to a wrapped group key: a fresh 24-byte XChaCha nonce.
const WRAP_NONCE_BYTES: usize = 24;

fn cipher(key: &[u8; PAIR_KEY_BYTES]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

/// One pairing attempt, either side. Different concrete data going in (only the initiator calls
/// [`PairingSession::offer`]; only the joiner calls [`PairingSession::accept`]) but the same shape
/// once handshaken, so confirmation and key transfer are identical from here on.
pub struct PairingSession {
    own_device: DeviceId,
    group: GroupId,
    nonce: Nonce,
    own_secret: Option<EphemeralSecret>,
    own_public: [u8; X25519_PUBLIC_KEY_BYTES],
    peer_device: Option<DeviceId>,
    handshake: Option<Handshake>,
    local_confirmed: bool,
    remote_confirmed: bool,
    failed_confirmations: u32,
    closed: bool,
}

struct Handshake {
    transcript: [u8; TRANSCRIPT_BYTES],
    shared_secret: [u8; X25519_PUBLIC_KEY_BYTES],
}

impl std::fmt::Debug for PairingSession {
    /// Never the shared secret or the ephemeral private key; only debugging-relevant state.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingSession")
            .field("own_device", &self.own_device)
            .field("peer_device", &self.peer_device)
            .field("handshaken", &self.handshake.is_some())
            .field("local_confirmed", &self.local_confirmed)
            .field("remote_confirmed", &self.remote_confirmed)
            .field("failed_confirmations", &self.failed_confirmations)
            .field("closed", &self.closed)
            .finish()
    }
}

fn fresh_nonce() -> Result<Nonce, PairingError> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| PairingError::Seal)?;
    Ok(nonce)
}

impl PairingSession {
    /// Starts a pairing as the initiator: keypair + nonce, registered, returned as the offer to
    /// render (`offer_inner` split out for the cognitive-complexity budget).
    #[tracing::instrument(skip_all, fields(device = %own_device, group = ?group))]
    pub fn offer(
        own_device: DeviceId,
        group: GroupId,
        endpoint: String,
        now_ms: u64,
        registry: &mut NonceRegistry,
    ) -> Result<(PairingSession, PairingOffer), PairingError> {
        let r = Self::offer_inner(own_device, group, endpoint, now_ms, registry);
        log_pairing_result("offer", r.as_ref().err().copied());
        r
    }

    fn offer_inner(
        own_device: DeviceId,
        group: GroupId,
        endpoint: String,
        now_ms: u64,
        registry: &mut NonceRegistry,
    ) -> Result<(PairingSession, PairingOffer), PairingError> {
        let own_secret = EphemeralSecret::random_from_rng(OsRng);
        let own_public = PublicKey::from(&own_secret).to_bytes();
        let nonce = fresh_nonce()?;
        registry.issue(nonce, now_ms)?;
        let offer = PairingOffer {
            device: own_device,
            group,
            public_key: own_public,
            endpoint,
            nonce,
            issued_at_ms: now_ms,
            // Filled in later by the daemon crate's pair_offer_impl (plan M8
            // sync-pairing-relay) when a relay is configured/bound; this crate's own crypto
            // core has no relay endpoint to read.
            relay_node_id: None,
            relay_url: None,
        };
        let session = PairingSession {
            own_device,
            group,
            nonce,
            own_secret: Some(own_secret),
            own_public,
            peer_device: None,
            handshake: None,
            local_confirmed: false,
            remote_confirmed: false,
            failed_confirmations: 0,
            closed: false,
        };
        Ok((session, offer))
    }

    /// Accepts an offer as the joiner: consumes the offer's nonce (single-use either way),
    /// completes the ECDH immediately, returns this device's own public key to send back.
    /// Wrapper/inner split, same reason as `offer`.
    #[tracing::instrument(skip_all, fields(device = %own_device, peer = %offer.device))]
    pub fn accept(
        own_device: DeviceId,
        offer: &PairingOffer,
        now_ms: u64,
        registry: &mut NonceRegistry,
    ) -> Result<(PairingSession, [u8; X25519_PUBLIC_KEY_BYTES]), PairingError> {
        let r = Self::accept_inner(own_device, offer, now_ms, registry);
        log_pairing_result("accept", r.as_ref().err().copied());
        r
    }

    fn accept_inner(
        own_device: DeviceId,
        offer: &PairingOffer,
        now_ms: u64,
        registry: &mut NonceRegistry,
    ) -> Result<(PairingSession, [u8; X25519_PUBLIC_KEY_BYTES]), PairingError> {
        registry.witness(offer.nonce, offer.issued_at_ms, now_ms)?;
        let own_secret = EphemeralSecret::random_from_rng(OsRng);
        let own_public = PublicKey::from(&own_secret).to_bytes();
        let peer_public = PublicKey::from(offer.public_key);
        let shared_secret = own_secret.diffie_hellman(&peer_public).to_bytes();
        let t = transcript(
            PROTOCOL_VERSION,
            Party {
                device: own_device,
                public_key: own_public,
            },
            Party {
                device: offer.device,
                public_key: offer.public_key,
            },
            offer.group,
        );
        let session = PairingSession {
            own_device,
            group: offer.group,
            nonce: offer.nonce,
            own_secret: None,
            own_public,
            peer_device: Some(offer.device),
            handshake: Some(Handshake {
                transcript: t,
                shared_secret,
            }),
            local_confirmed: false,
            remote_confirmed: false,
            failed_confirmations: 0,
            closed: false,
        };
        Ok((session, own_public))
    }

    /// Completes the handshake as the initiator once the joiner's public key comes back; consumes
    /// this session's own nonce in `registry`, independent of the joiner's own `accept` consume.
    pub fn complete(
        &mut self,
        peer_device: DeviceId,
        peer_public: [u8; X25519_PUBLIC_KEY_BYTES],
        now_ms: u64,
        registry: &mut NonceRegistry,
    ) -> Result<(), PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        registry.consume(self.nonce, now_ms)?;
        let own_secret = self.own_secret.take().ok_or(PairingError::NotHandshaken)?;
        let shared_secret = own_secret
            .diffie_hellman(&PublicKey::from(peer_public))
            .to_bytes();
        let t = transcript(
            PROTOCOL_VERSION,
            Party {
                device: self.own_device,
                public_key: self.own_public,
            },
            Party {
                device: peer_device,
                public_key: peer_public,
            },
            self.group,
        );
        self.peer_device = Some(peer_device);
        self.handshake = Some(Handshake {
            transcript: t,
            shared_secret,
        });
        debug_assert!(self.handshake.is_some());
        debug_assert!(self.peer_device.is_some());
        Ok(())
    }

    /// The six words this device should show for human comparison. `Err(NotHandshaken)` before
    /// [`PairingSession::complete`] (initiator) or immediately after [`PairingSession::accept`].
    pub fn sas_words(&self) -> Result<[&'static str; SAS_WORD_COUNT], PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        let hs = self.handshake.as_ref().ok_or(PairingError::NotHandshaken)?;
        Ok(sas_words(&hs.shared_secret, &hs.transcript)?)
    }

    /// Records this device's human pressing "yes, the words match".
    pub fn confirm_local(&mut self) -> Result<(), PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        if self.handshake.is_none() {
            return Err(PairingError::NotHandshaken);
        }
        self.local_confirmed = true;
        Ok(())
    }

    /// Records that the peer's confirmation arrived (however the transport carries it).
    pub fn confirm_remote(&mut self) -> Result<(), PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        if self.handshake.is_none() {
            return Err(PairingError::NotHandshaken);
        }
        self.remote_confirmed = true;
        Ok(())
    }

    /// Records a failed/mismatched confirmation (human said "no", or the peer's never arrived).
    /// Closes the window outright at [`MAX_FAILED_SAS_CONFIRMATIONS`]: rate-limit, then stop.
    pub fn reject(&mut self) -> Result<(), PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        self.failed_confirmations += 1;
        if self.failed_confirmations >= MAX_FAILED_SAS_CONFIRMATIONS {
            self.closed = true;
        }
        debug_assert!(self.failed_confirmations <= MAX_FAILED_SAS_CONFIRMATIONS);
        Ok(())
    }

    /// True once both sides have confirmed and the window is not closed — the only state in which
    /// the group key may move.
    pub fn is_ready_to_send_key(&self) -> bool {
        self.local_confirmed && self.remote_confirmed && !self.closed
    }

    /// Seals `group_key_bytes` under the transcript-derived key-wrap key. Refuses outside
    /// [`PairingSession::is_ready_to_send_key`] (a one-sided confirmation transfers nothing), with
    /// [`PairingError::Closed`] taking precedence over merely-unconfirmed.
    pub fn wrap_group_key(&self, group_key_bytes: &[u8]) -> Result<Vec<u8>, PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        if !self.is_ready_to_send_key() {
            return Err(PairingError::NotConfirmed);
        }
        let hs = self.handshake.as_ref().ok_or(PairingError::NotHandshaken)?;
        let key = pair_key(&hs.shared_secret, &hs.transcript)?;
        let mut nonce = [0u8; WRAP_NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| PairingError::Seal)?;
        let ciphertext = cipher(&key)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: group_key_bytes,
                    aad: &hs.transcript,
                },
            )
            .map_err(|_| PairingError::Seal)?;
        let mut out = Vec::with_capacity(WRAP_NONCE_BYTES + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Opens a group key sealed by [`PairingSession::wrap_group_key`]. Refuses outside
    /// [`PairingSession::is_ready_to_send_key`]: a one-sided confirmation accepts nothing either.
    pub fn unwrap_group_key(&self, sealed: &[u8]) -> Result<Vec<u8>, PairingError> {
        if self.closed {
            return Err(PairingError::Closed);
        }
        if !self.is_ready_to_send_key() {
            return Err(PairingError::NotConfirmed);
        }
        if sealed.len() < WRAP_NONCE_BYTES {
            return Err(PairingError::Seal);
        }
        let hs = self.handshake.as_ref().ok_or(PairingError::NotHandshaken)?;
        let key = pair_key(&hs.shared_secret, &hs.transcript)?;
        let (nonce, ciphertext) = sealed.split_at(WRAP_NONCE_BYTES);
        cipher(&key)
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &hs.transcript,
                },
            )
            .map_err(|_| PairingError::Seal)
    }

    /// [`PairingSession::wrap_group_key`] for the normative payload: the group key **and** this
    /// device's long-term static public key, so a caller cannot send one without the other.
    pub fn wrap_grant(&self, grant: &PairingGrant) -> Result<Vec<u8>, PairingError> {
        let bytes = grant.to_bytes().map_err(|_| PairingError::Seal)?;
        self.wrap_group_key(&bytes)
    }

    /// [`PairingSession::unwrap_group_key`] decoded back into a [`PairingGrant`].
    pub fn unwrap_grant(&self, sealed: &[u8]) -> Result<PairingGrant, PairingError> {
        let bytes = self.unwrap_group_key(sealed)?;
        PairingGrant::from_bytes(&bytes).map_err(|_| PairingError::Seal)
    }

    /// The peer's device id, once the handshake has identified it.
    pub fn peer_device(&self) -> Option<DeviceId> {
        self.peer_device
    }

    /// The sync group this handshake is for — a real relay (`pairing_lan.rs`) validates an
    /// incoming peer's claimed group against this before ever touching handshake state.
    pub fn group(&self) -> GroupId {
        self.group
    }

    /// The offer's one-time pairing nonce this handshake is bound to, so a relay can confirm an
    /// incoming attempt is answering *this* offer and not a stale or foreign one.
    pub fn nonce(&self) -> Nonce {
        self.nonce
    }

    /// Whether the ECDH has completed: always true for the joiner (immediately, in
    /// [`PairingSession::accept`]); only after [`PairingSession::complete`] for the initiator.
    pub fn is_handshaken(&self) -> bool {
        self.handshake.is_some()
    }

    /// Whether this device's own human has confirmed the SAS.
    pub fn is_locally_confirmed(&self) -> bool {
        self.local_confirmed
    }
}

/// Shared by `offer`/`accept`, split out so the event macro doesn't count against either
/// `#[instrument]` budget. `warn!`, not `debug!`, on failure: pairing is a rare, human-paced
/// ceremony, not routine traffic, so every refusal here is worth a human's attention
/// (`tasks/logging-sync-crate/notes.md`). `PairingError` is `Copy` and its `Display` is confirmed
/// payload-free, so logging it directly is safe.
fn log_pairing_result(op: &'static str, err: Option<PairingError>) {
    match err {
        None => log_pairing_ok(op),
        Some(e) => log_pairing_failed(op, e),
    }
}

fn log_pairing_ok(op: &'static str) {
    tracing::debug!(op, "pairing_step_ok");
}

fn log_pairing_failed(op: &'static str, e: PairingError) {
    tracing::warn!(op, error = %e, "pairing_step_failed");
}
