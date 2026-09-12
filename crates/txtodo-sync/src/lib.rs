//! Sync protocol (plan M4): the frozen `Frame` envelope, the `Hello`/`Want`/`Ops`/`Ack` messages,
//! head diffing and the session state machine. Transport-agnostic: frames in, frames out, no
//! sockets here. Crypto is the per-op signature (`sign`) and the per-batch AEAD (`aead`);
//! transports are the mDNS/iroh task.
#![forbid(unsafe_code)]

mod aead;
mod crypto_error;
mod frame;
mod message;
mod session;
mod session_error;
mod sign;
mod want;

pub use aead::{
    AAD_BYTES, GroupKey, GroupKeys, KEY_BYTES, MAX_RETAINED_KEY_EPOCHS, NONCE_BYTES,
    SEALED_HEADER_BYTES, TAG_BYTES, open, seal,
};
pub use crypto_error::CryptoError;
pub use frame::{Frame, FrameError, HEADER_BYTES, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION};
pub use message::{
    GroupId, Heads, MAX_HEADS, MAX_OPS_PER_BATCH, MAX_WANT_RANGES, Message, MessageError,
    OriginRange,
};
pub use session::{Greeting, Session, SessionState};
pub use session_error::SessionError;
pub use sign::{
    DevicePublicKey, DeviceSigningKey, PUBLIC_KEY_BYTES, SIGNATURE_BYTES, SIGNING_KEY_BYTES,
    Signature, sign, verify, verify_batch,
};
pub use want::{Gap, advance, want};

#[cfg(test)]
mod aead_tests;
#[cfg(test)]
mod crypto_error_tests;
#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod message_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod sign_tests;
#[cfg(test)]
mod want_tests;
