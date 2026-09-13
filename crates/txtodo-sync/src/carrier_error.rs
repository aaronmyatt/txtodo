//! Typed error for the file-carrier transport (`append_frame.rs`, `carrier.rs`; plan M8 §4.5).
//! Every variant names what was attempted (CLAUDE.md §3) — nothing here is an assertion, because
//! every byte and file name under the sync directory that this device did not just write itself is
//! external input (a peer's file, a stale rotation, a partially-synced copy).

use std::fmt;
use std::path::PathBuf;

use txtodo_model::DeviceId;

use crate::frame::FrameError;
use crate::link::LinkError;

/// Why the file carrier could not send, receive, or set itself up.
#[derive(Debug)]
pub enum CarrierError {
    /// An append-frame's length exceeds [`crate::append_frame::MAX_APPEND_FRAME_BYTES`] — genuine
    /// corruption, never a partial write (see [`crate::append_frame::AppendFrame::try_decode`]).
    FrameTooLarge {
        /// The claimed length.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// An append-frame's body did not decode as a wire [`crate::Frame`].
    Frame(FrameError),
    /// A write was about to land in a file that does not belong to this device — refused before
    /// touching it (design §4.5's "own-file-only" invariant: one device can never write another
    /// device's file).
    ForeignDevice {
        /// The file that was about to be written.
        path: PathBuf,
        /// The device the file name actually belongs to.
        owner: DeviceId,
        /// This carrier's own device.
        us: DeviceId,
    },
    /// A file under the sync directory did not parse as `<device-id>[-<n>].ops`.
    BadFileName {
        /// The offending path.
        path: PathBuf,
    },
    /// The sync directory itself could not be created or listed, or is not a real directory.
    Dir {
        /// The directory.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
    /// A read or write on one `.ops` file failed.
    Io {
        /// The file.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
}

impl fmt::Display for CarrierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CarrierError::FrameTooLarge { len, max } => {
                write!(f, "append-frame of {len} bytes exceeds the {max} byte cap")
            }
            CarrierError::Frame(e) => write!(f, "append-frame body did not decode: {e}"),
            CarrierError::ForeignDevice { path, owner, us } => write!(
                f,
                "refused to write {} (belongs to device {owner}, this carrier is device {us})",
                path.display()
            ),
            CarrierError::BadFileName { path } => {
                write!(
                    f,
                    "{} is not a <device-id>[-<n>].ops file name",
                    path.display()
                )
            }
            CarrierError::Dir { path, message } => {
                write!(f, "sync directory {}: {message}", path.display())
            }
            CarrierError::Io { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for CarrierError {}

impl From<CarrierError> for LinkError {
    fn from(e: CarrierError) -> LinkError {
        match e {
            CarrierError::Frame(fe) => LinkError::Frame(fe),
            other => LinkError::Io(other.to_string()),
        }
    }
}
