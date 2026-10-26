//! `WebError`: why a command failed. Its text is what the frontend shows, the same words the
//! daemon's own errors use where there is one (`MutationError`, `StateError`).

use txtodo_engine::mutation::MutationError;
use txtodo_engine::state_error::StateError;
use txtodo_model::HlcError;
use txtodo_store::StoreError;

/// Why a command failed.
#[derive(Debug)]
pub enum WebError {
    /// The op log refused or failed a read or write.
    Store(StoreError),
    /// An op did not fit the document.
    State(StateError),
    /// A mutation could not become ops.
    Mutation(MutationError),
    /// The clock could not advance.
    Clock(HlcError),
    /// A command's arguments did not parse; names which.
    BadArgs(String),
    /// A command this host does not answer yet.
    UnknownCommand(String),
    /// Something this host refuses for now; names what.
    Unsupported(&'static str),
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebError::Store(e) => write!(f, "store: {e}"),
            WebError::State(e) => write!(f, "{e}"),
            WebError::Mutation(e) => write!(f, "{e}"),
            WebError::Clock(e) => write!(f, "clock: {e}"),
            WebError::BadArgs(what) => write!(f, "bad arguments: {what}"),
            WebError::UnknownCommand(cmd) => write!(f, "the web host does not answer `{cmd}` yet"),
            WebError::Unsupported(what) => write!(f, "{what} is not supported in the browser yet"),
        }
    }
}

impl std::error::Error for WebError {}

impl From<StoreError> for WebError {
    fn from(e: StoreError) -> WebError {
        WebError::Store(e)
    }
}

impl From<StateError> for WebError {
    fn from(e: StateError) -> WebError {
        WebError::State(e)
    }
}

impl From<MutationError> for WebError {
    fn from(e: MutationError) -> WebError {
        WebError::Mutation(e)
    }
}

impl From<HlcError> for WebError {
    fn from(e: HlcError) -> WebError {
        WebError::Clock(e)
    }
}
