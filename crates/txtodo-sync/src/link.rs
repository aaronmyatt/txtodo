//! The narrow seam between the session state machine and an actual network. `txtodo-sync` owns
//! `Session` and must stay testable without sockets ([sync-lan-transport] task notes), so every
//! transport — the real iroh/mDNS LAN link, the M8 file-carrier, and the in-process
//! [`ChannelLink`] tests and the simulator use — is just an implementation of this trait. Nothing
//! outside a `Link` implementation may assume frames arrive over a socket.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::frame::{Frame, FrameError};

/// Frames queued for one direction of a [`ChannelLink`] before the sender is refused. Matches this
/// workspace's rule that every collection has a named, checked cap.
pub const MAX_QUEUED_FRAMES: usize = 256;

/// One side of a bidirectional frame stream. `Send` so a link can be handed to a task/thread that
/// drives a `Session`; nothing here is `Sync` — one link, one driver.
pub trait Link: Send {
    /// Sends one frame. May block until the peer (or an internal buffer) can accept it.
    fn send(&mut self, frame: Frame) -> Result<(), LinkError>;
    /// Blocks for the next frame from the peer.
    fn recv(&mut self) -> Result<Frame, LinkError>;
    /// Ends this side's sends and blocks until the peer has acknowledged every frame sent so far
    /// (bounded by the link's own idle timeout). Call it before dropping a link whose *last*
    /// `send` must actually arrive: dropping an `IrohLink` closes its QUIC connection at once and
    /// may discard bytes still queued (that lost every pairing reply over a real relay,
    /// 2026-09-22). In-process links deliver on `send`, so the default is a no-op.
    fn finish(&mut self) -> Result<(), LinkError> {
        Ok(())
    }
}

/// Why a `Link` operation failed. Every variant names what was attempted, never assumes the caller
/// already knows (CLAUDE.md §3).
#[derive(Debug)]
pub enum LinkError {
    /// The peer closed its side; no more frames will ever arrive.
    Closed,
    /// A frame that arrived did not decode.
    Frame(FrameError),
    /// The underlying transport reported an I/O error (real transports only).
    Io(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::Closed => write!(f, "the peer closed this link"),
            LinkError::Frame(e) => write!(f, "a frame on this link did not decode: {e}"),
            LinkError::Io(reason) => write!(f, "link I/O error: {reason}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// One direction's state, behind one lock: the pending frames and whether the sender is gone.
/// Keeping both under a single `Mutex` (rather than a separate flag mutex) removes any lock-order
/// question between "is it closed" and "what is queued" — there is only ever one lock to hold.
struct QueueState {
    frames: VecDeque<Frame>,
    closed: bool,
}

struct Queue {
    state: Mutex<QueueState>,
    not_empty: Condvar,
}

impl Queue {
    fn new() -> Queue {
        Queue {
            state: Mutex::new(QueueState {
                frames: VecDeque::new(),
                closed: false,
            }),
            not_empty: Condvar::new(),
        }
    }

    fn push(&self, frame: Frame) -> Result<(), LinkError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return Err(LinkError::Closed);
        }
        // A full queue means the receiver stopped draining; refusing rather than growing without
        // bound is the same policy every other wire collection in this crate follows.
        if state.frames.len() >= MAX_QUEUED_FRAMES {
            return Err(LinkError::Io(format!(
                "queue full at {MAX_QUEUED_FRAMES} frames"
            )));
        }
        state.frames.push_back(frame);
        debug_assert!(state.frames.len() <= MAX_QUEUED_FRAMES);
        drop(state);
        self.not_empty.notify_one();
        Ok(())
    }

    fn pop(&self) -> Result<Frame, LinkError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(frame) = state.frames.pop_front() {
                return Ok(frame);
            }
            if state.closed {
                return Err(LinkError::Closed);
            }
            state = self
                .not_empty
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        drop(state);
        self.not_empty.notify_all();
    }
}

/// An in-process, in-memory `Link`: what the loopback tests and the simulator use instead of a
/// socket. Two of these, built by [`channel_link_pair`], talk only to each other.
pub struct ChannelLink {
    outbox: Arc<Queue>,
    inbox: Arc<Queue>,
}

impl Link for ChannelLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        self.outbox.push(frame)
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        self.inbox.pop()
    }
}

impl Drop for ChannelLink {
    fn drop(&mut self) {
        // Closing our outbox is how the peer's `recv` learns we are gone rather than blocking
        // forever on a link nobody will ever write to again.
        self.outbox.close();
    }
}

/// Builds two [`ChannelLink`]s wired to each other: whatever one sends, the other receives.
pub fn channel_link_pair() -> (ChannelLink, ChannelLink) {
    let a_to_b = Arc::new(Queue::new());
    let b_to_a = Arc::new(Queue::new());
    let a = ChannelLink {
        outbox: Arc::clone(&a_to_b),
        inbox: Arc::clone(&b_to_a),
    };
    let b = ChannelLink {
        outbox: b_to_a,
        inbox: a_to_b,
    };
    (a, b)
}
