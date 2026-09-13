//! Sync protocol (plan M4): the frozen `Frame` envelope, the `Hello`/`Want`/`Ops`/`Ack` messages,
//! head diffing and the session state machine. Transport-agnostic: frames in, frames out, no
//! sockets here. Crypto is the per-op signature (`sign`) and the per-batch AEAD (`aead`);
//! transports are the mDNS/iroh task.
#![forbid(unsafe_code)]

mod aead;
mod crypto_error;
mod device_static;
mod discovery;
mod eff_wordlist;
mod endpoint;
mod frame;
mod keystore;
mod keystore_error;
mod keystore_file;
mod keystore_memory;
mod keystore_os;
mod keystore_resolve;
mod lan_link;
mod lan_op_signing;
mod link;
mod message;
mod nonce_registry;
mod offer;
mod pairing;
mod pairing_error;
mod pairing_grant;
mod rotation;
mod rotation_error;
mod sas;
mod sealed_ops;
mod session;
mod session_error;
mod sign;
mod transcript;
mod want;

pub use aead::{
    AAD_BYTES, GroupKey, GroupKeys, KEY_BYTES, MAX_RETAINED_KEY_EPOCHS, NONCE_BYTES,
    SEALED_HEADER_BYTES, TAG_BYTES, open, seal,
};
pub use crypto_error::CryptoError;
pub use device_static::{DEVICE_STATIC_KEY_BYTES, DeviceStaticPublic, DeviceStaticSecret};
pub use discovery::{
    Announcement, AnnouncementError, BrowseEvents, DEBOUNCE_MS, DiscoveredPeer, Discovery,
    DiscoveryError, Ignored, MAX_BACKOFF_MS, MAX_LAN_PEERS, PeerEvent, PeerTable, SERVICE_TYPE,
    Sighting, TXT_DEVICE, TXT_GROUP, TXT_NODE, TXT_PROTO, backoff_ms, parse_announcement,
};
pub use eff_wordlist::{WORDLIST_LEN, WORDLIST_SHA256, wordlist};
pub use endpoint::{ALPN, bind_local_endpoint};
pub use frame::{Frame, FrameError, HEADER_BYTES, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION};
pub use keystore::{KeyId, KeyStore, MAX_STORED_EPOCHS, Secret};
pub use keystore_error::KeyStoreError;
pub use keystore_file::{ARGON2_ITERATIONS, ARGON2_MEMORY_KIB, ARGON2_PARALLELISM, FileKeyStore};
pub use keystore_memory::MemoryKeyStore;
pub use keystore_os::OsKeyStore;
pub use keystore_resolve::{KeyStoreMode, ResolvedBackend, resolve};
pub use lan_link::{IrohLink, LanEndpoint, LanError};
pub use lan_op_signing::{LAN_OP_SIGN_INFO, derive_group_op_signing_key};
pub use link::{ChannelLink, Link, LinkError, MAX_QUEUED_FRAMES, channel_link_pair};
pub use message::{
    GroupId, Heads, MAX_HEADS, MAX_OPS_PER_BATCH, MAX_WANT_RANGES, Message, MessageError,
    OriginRange,
};
pub use nonce_registry::{
    MAX_CONCURRENT_PAIRINGS, Nonce, NonceError, NonceRegistry, PAIRING_WINDOW_MS,
};
pub use offer::{OfferError, PairingOffer, from_code, from_qr_bytes, to_code, to_qr_bytes};
pub use pairing::{MAX_FAILED_SAS_CONFIRMATIONS, PairingSession};
pub use pairing_error::PairingError;
pub use pairing_grant::{PairingGrant, PairingGrantError};
pub use rotation::{
    GRANT_INFO, WrappedGrant, open_grant, plan_rotation, validate_removal, wrap_grant_for,
};
pub use rotation_error::{RemovalError, RotationError};
pub use sas::{
    PAIR_KEY_BYTES, PAIR_KEY_INFO, SAS_INFO, SAS_WORD_COUNT, SasError, pair_key, sas_words,
};
pub use sealed_ops::{SealContext, SealedOpsError, open_ops, seal_ops};
pub use session::{Greeting, Session, SessionState};
pub use session_error::SessionError;
pub use sign::{
    DevicePublicKey, DeviceSigningKey, PUBLIC_KEY_BYTES, SIGNATURE_BYTES, SIGNING_KEY_BYTES,
    Signature, sign, verify, verify_batch,
};
pub use transcript::{TRANSCRIPT_BYTES, X25519_PUBLIC_KEY_BYTES, transcript};
pub use want::{Gap, advance, want};

#[cfg(test)]
mod aead_tests;
#[cfg(test)]
mod crypto_error_tests;
#[cfg(test)]
mod device_static_tests;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod eff_wordlist_tests;
#[cfg(test)]
mod endpoint_tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod keystore_file_tests;
#[cfg(test)]
mod keystore_memory_tests;
#[cfg(test)]
mod keystore_resolve_tests;
#[cfg(test)]
mod keystore_tests;
#[cfg(test)]
mod link_tests;
#[cfg(test)]
mod message_tests;
#[cfg(test)]
mod nonce_registry_tests;
#[cfg(test)]
mod offer_tests;
#[cfg(test)]
mod pairing_grant_tests;
#[cfg(test)]
mod pairing_tests;
#[cfg(test)]
mod rotation_tests;
#[cfg(test)]
mod sas_tests;
#[cfg(test)]
mod sealed_ops_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod sign_tests;
#[cfg(test)]
mod transcript_tests;
#[cfg(test)]
mod want_tests;
