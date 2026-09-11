//! The one error type every command returns; `main` prints it and sets the exit status.

use crate::{client, config, store};
use std::fmt;

/// Anything that ends the run with a message on stderr and exit status 1.
#[derive(Debug)]
pub enum CliError {
    /// The config file exists but is unusable.
    Config(config::ConfigError),
    /// The process environment or the random source could not be read.
    Io(std::io::Error),
    /// A file could not be read or written.
    Store(store::StoreError),
    /// An edit argument the core rejects.
    Edit(txtodo_core::EditError),
    /// Wrong arguments; the value is the todo.sh usage line.
    Usage(&'static str),
    /// A todo.sh-worded failure, printed as is.
    Message(String),
    /// Already printed to stderr by the command; only the exit status remains.
    Reported,
    /// Daemon mode failed (socket refused, RPC error).
    Client(client::ClientError),
}

impl From<client::ClientError> for CliError {
    fn from(e: client::ClientError) -> CliError {
        CliError::Client(e)
    }
}

impl From<store::StoreError> for CliError {
    fn from(e: store::StoreError) -> CliError {
        CliError::Store(e)
    }
}
impl From<txtodo_core::EditError> for CliError {
    fn from(e: txtodo_core::EditError) -> CliError {
        CliError::Edit(e)
    }
}
impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> CliError {
        CliError::Io(e)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Config(e) => write!(f, "{e}"),
            CliError::Io(e) => write!(f, "{e}"),
            CliError::Store(e) => write!(f, "{e}"),
            CliError::Edit(e) => write!(f, "{e}"),
            CliError::Usage(u) => write!(f, "usage: txtodo {u}"),
            CliError::Message(m) => write!(f, "{m}"),
            CliError::Reported => Ok(()),
            CliError::Client(e) => write!(f, "{e}"),
        }
    }
}
