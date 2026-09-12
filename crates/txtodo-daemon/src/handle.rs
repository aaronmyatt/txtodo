//! The actor's public face: messages, the cloneable handle clients hold, and the reply types.
//! One bounded mailbox per document; senders await backpressure, nothing is dropped.
//! Ref: https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html

use crate::expected::Hash;
use crate::mutation::{Mutation, MutationError, TaskRef};
use crate::state::StateError;
use crate::write::WriteError;
use std::fmt;
use tokio::sync::{broadcast, mpsc, oneshot};
use txtodo_model::{DeviceId, FilePath, Hlc, HlcError, Principal, TaskId};
use txtodo_store::{ReviewRow, StoreError, Stored};

/// Mailbox depth per document.
pub const ACTOR_MAILBOX_CAP: usize = 256;
/// Change events buffered per subscriber before it is told it lagged.
pub const WATCH_CAP: usize = 64;

/// A document's current bytes and hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contents {
    /// Exact projection bytes.
    pub bytes: Vec<u8>,
    /// blake3 of `bytes`.
    pub hash: Hash,
}

/// What an Apply or Undo produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Ops appended.
    pub applied: u32,
    /// Projection hash afterwards.
    pub hash: Hash,
    /// The batch's stamp.
    pub hlc: Hlc,
}

/// One change to one document, broadcast to Watch subscribers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The document.
    pub path: FilePath,
    /// Projection hash afterwards.
    pub hash: Hash,
    /// The ops appended, with their seqs.
    pub ops: Vec<Stored>,
    /// needs_review flags this change raised (an import), if any.
    pub review: Vec<ReviewRow>,
}

/// Which side a resolution keeps. Closed set; mirrors the wire enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// This device's text at flag time.
    Mine,
    /// The peer's text at flag time.
    Theirs,
    /// What is in the file now; only the flag is cleared.
    Merged,
}

/// An open flag with the line its task sits on now (0 when it left the file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    /// The stored flag.
    pub row: ReviewRow,
    /// 1-based line, 0 when the task is no longer in the file.
    pub line_number: usize,
}

/// Everything the actor can fail with. Client errors (`Mutation`) map to gRPC InvalidArgument /
/// FailedPrecondition; the rest are Internal.
#[derive(Debug)]
pub enum ActorError {
    /// A mutation was refused.
    Mutation(MutationError),
    /// An op did not apply (a daemon bug or a stale client racing an external edit).
    State(StateError),
    /// The store failed.
    Store(StoreError),
    /// The file could not be read or written.
    Write(WriteError),
    /// HLC counter overflow.
    Hlc(HlcError),
    /// The Loro mirror could not be built when the actor opened (a bug, not a client error).
    Mirror(String),
    /// A resolution named a task with no open needs_review flag.
    NoFlag(TaskId),
    /// The actor task has stopped.
    Gone(FilePath),
    /// Not available on one device in M3.
    Unsupported(&'static str),
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActorError::Mutation(e) => write!(f, "{e}"),
            ActorError::State(e) => write!(f, "apply op: {e}"),
            ActorError::Store(e) => write!(f, "store: {e}"),
            ActorError::Write(e) => write!(f, "{e}"),
            ActorError::Hlc(e) => write!(f, "{e}"),
            ActorError::Mirror(m) => write!(f, "mirror: {m}"),
            ActorError::NoFlag(t) => write!(f, "no open needs_review flag for task {t}"),
            ActorError::Gone(p) => write!(f, "actor for {p} has stopped"),
            ActorError::Unsupported(what) => write!(f, "{what} is not supported yet"),
        }
    }
}

impl std::error::Error for ActorError {}

impl From<MutationError> for ActorError {
    fn from(e: MutationError) -> ActorError {
        ActorError::Mutation(e)
    }
}
impl From<StateError> for ActorError {
    fn from(e: StateError) -> ActorError {
        ActorError::State(e)
    }
}
impl From<StoreError> for ActorError {
    fn from(e: StoreError) -> ActorError {
        ActorError::Store(e)
    }
}
impl From<WriteError> for ActorError {
    fn from(e: WriteError) -> ActorError {
        ActorError::Write(e)
    }
}
impl From<HlcError> for ActorError {
    fn from(e: HlcError) -> ActorError {
        ActorError::Hlc(e)
    }
}

/// Everything a client can ask the actor. Replies travel on one-shot channels.
pub enum ActorMsg {
    /// The watcher saw the file change.
    ExternalChange,
    /// Intent-level mutations from a client.
    Apply {
        /// What to do, in order; later ones see earlier ones applied.
        mutations: Vec<Mutation>,
        /// Who asks.
        principal: Principal,
        /// Result channel.
        reply: oneshot::Sender<Result<Applied, ActorError>>,
    },
    /// Current bytes.
    Get {
        /// Result channel.
        reply: oneshot::Sender<Contents>,
    },
    /// A stream of future changes.
    Subscribe {
        /// Result channel.
        reply: oneshot::Sender<broadcast::Receiver<Change>>,
    },
    /// Append inverse ops for the newest `steps` ops.
    Undo {
        /// How many ops to invert (0 = 1).
        steps: u16,
        /// Who asks.
        principal: Principal,
        /// Result channel.
        reply: oneshot::Sender<Result<Applied, ActorError>>,
    },
    /// The document as it was at a wall time.
    Checkout {
        /// Inclusive upper bound, Unix milliseconds.
        at_wall_ms: u64,
        /// Result channel.
        reply: oneshot::Sender<Result<Vec<u8>, ActorError>>,
    },
    /// A peer's Loro updates to merge (plan M4).
    Import {
        /// `LoroDocument::export_updates` bytes from the peer.
        updates: Vec<u8>,
        /// The peer.
        peer: DeviceId,
        /// Result channel.
        reply: oneshot::Sender<Result<Applied, ActorError>>,
    },
    /// The open needs_review flags.
    Conflicts {
        /// Result channel.
        reply: oneshot::Sender<Result<Vec<ConflictRow>, ActorError>>,
    },
    /// The mirror's version as opaque bytes (what a peer exports since).
    Version {
        /// Result channel.
        reply: oneshot::Sender<Vec<u8>>,
    },
    /// The Loro updates a peer at `since` is missing.
    Export {
        /// The peer's `Version` bytes.
        since: Vec<u8>,
        /// Result channel.
        reply: oneshot::Sender<Result<Vec<u8>, ActorError>>,
    },
    /// Resolve one flag.
    Resolve {
        /// The line.
        task: TaskRef,
        /// Which side.
        resolution: Resolution,
        /// Who asks.
        principal: Principal,
        /// Result channel.
        reply: oneshot::Sender<Result<Applied, ActorError>>,
    },
}

/// A cheap handle to one document's actor.
#[derive(Clone)]
pub struct ActorHandle {
    path: FilePath,
    tx: mpsc::Sender<ActorMsg>,
}

impl ActorHandle {
    pub(crate) fn new(path: FilePath, tx: mpsc::Sender<ActorMsg>) -> ActorHandle {
        ActorHandle { path, tx }
    }

    /// The document this handle addresses.
    pub fn path(&self) -> &FilePath {
        &self.path
    }

    async fn send(&self, msg: ActorMsg) -> Result<(), ActorError> {
        self.tx
            .send(msg)
            .await
            .map_err(|_| ActorError::Gone(self.path.clone()))
    }

    async fn ask<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<T>) -> ActorMsg,
    ) -> Result<T, ActorError> {
        let (tx, rx) = oneshot::channel();
        self.send(build(tx)).await?;
        rx.await.map_err(|_| ActorError::Gone(self.path.clone()))
    }

    /// Tells the actor the file changed on disk.
    pub async fn external_change(&self) -> Result<(), ActorError> {
        self.send(ActorMsg::ExternalChange).await
    }

    /// Applies mutations.
    pub async fn apply(
        &self,
        mutations: Vec<Mutation>,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        self.ask(|reply| ActorMsg::Apply {
            mutations,
            principal,
            reply,
        })
        .await?
    }

    /// Current bytes and hash.
    pub async fn get(&self) -> Result<Contents, ActorError> {
        self.ask(|reply| ActorMsg::Get { reply }).await
    }

    /// Future changes.
    pub async fn subscribe(&self) -> Result<broadcast::Receiver<Change>, ActorError> {
        self.ask(|reply| ActorMsg::Subscribe { reply }).await
    }

    /// Inverts the newest `steps` ops.
    pub async fn undo(&self, steps: u16, principal: Principal) -> Result<Applied, ActorError> {
        self.ask(|reply| ActorMsg::Undo {
            steps,
            principal,
            reply,
        })
        .await?
    }

    /// The document at a wall time.
    pub async fn checkout(&self, at_wall_ms: u64) -> Result<Vec<u8>, ActorError> {
        self.ask(|reply| ActorMsg::Checkout { at_wall_ms, reply })
            .await?
    }

    /// Merges a peer's Loro updates.
    pub async fn import_updates(
        &self,
        updates: Vec<u8>,
        peer: DeviceId,
    ) -> Result<Applied, ActorError> {
        self.ask(|reply| ActorMsg::Import {
            updates,
            peer,
            reply,
        })
        .await?
    }

    /// The open needs_review flags with their current lines.
    pub async fn conflicts(&self) -> Result<Vec<ConflictRow>, ActorError> {
        self.ask(|reply| ActorMsg::Conflicts { reply }).await?
    }

    /// The mirror's version as opaque bytes.
    pub async fn version(&self) -> Result<Vec<u8>, ActorError> {
        self.ask(|reply| ActorMsg::Version { reply }).await
    }

    /// The Loro updates a peer at `since` is missing.
    pub async fn export_since(&self, since: Vec<u8>) -> Result<Vec<u8>, ActorError> {
        self.ask(|reply| ActorMsg::Export { since, reply }).await?
    }

    /// Resolves one flag.
    pub async fn resolve(
        &self,
        task: TaskRef,
        resolution: Resolution,
        principal: Principal,
    ) -> Result<Applied, ActorError> {
        self.ask(|reply| ActorMsg::Resolve {
            task,
            resolution,
            principal,
            reply,
        })
        .await?
    }
}
