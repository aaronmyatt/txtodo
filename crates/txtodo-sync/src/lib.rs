//! Sync protocol (plan M4): the frozen `Frame` envelope, the `Hello`/`Want`/`Ops`/`Ack` messages,
//! head diffing and the session state machine. Transport-agnostic: frames in, frames out, no
//! sockets here. Crypto is the envelope task; transports are the mDNS/iroh task.
#![forbid(unsafe_code)]

mod frame;

pub use frame::{Frame, FrameError, HEADER_BYTES, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION};

#[cfg(test)]
mod frame_tests;
