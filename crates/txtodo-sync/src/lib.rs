//! Sync protocol (plan M4): the frozen `Frame` envelope, the `Hello`/`Want`/`Ops`/`Ack` messages,
//! head diffing and the session state machine. Transport-agnostic: frames in, frames out, no
//! sockets here. Crypto is the envelope task; transports are the mDNS/iroh task.
#![forbid(unsafe_code)]

mod frame;
mod message;

pub use frame::{Frame, FrameError, HEADER_BYTES, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION};
pub use message::{
    GroupId, Heads, MAX_HEADS, MAX_OPS_PER_BATCH, MAX_WANT_RANGES, Message, MessageError,
    OriginRange,
};

#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod message_tests;
