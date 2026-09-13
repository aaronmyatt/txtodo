//! Why a [`crate::bundle_import::import_from_chunks`] call failed. Split out of `bundle_import.rs`
//! to keep that file within its line budget, the same pattern as `workspace_error.rs`.

use crate::bundle_crypto::BundleCryptoError;
use crate::bundle_wire::RecordReadError;
use crate::write::WriteError;

/// Why an import failed. Each variant names its own stage — the point of the exercise, per design
/// §4.5's edge cases ("a distinct stage error", never one generic failure).
#[derive(Debug)]
pub(crate) enum BundleImportError {
    /// The stream ended before a whole header/manifest/record was read.
    Truncated,
    /// The clear header failed to decode.
    Header(BundleCryptoError),
    /// The clear manifest's protobuf bytes did not decode.
    ManifestDecode(prost::DecodeError),
    /// `BundleManifest.version` is not one this build speaks.
    UnsupportedVersion(u32),
    /// `BundleManifest.schema_version` does not match this store's own.
    SchemaMismatch {
        /// What the manifest claimed.
        found: String,
        /// What this workspace's store actually holds.
        supported: String,
    },
    /// The AEAD layer failed: wrong passphrase, or the bundle is corrupt (indistinguishable by
    /// construction — see [`BundleCryptoError::Aead`]'s own doc).
    Decrypt(BundleCryptoError),
    /// The decrypted record stream was malformed or truncated.
    Record(RecordReadError),
    /// A stored path is not a valid workspace-relative `FilePath`.
    BadPath(String),
    /// The files actually received do not match the manifest's own file list.
    FileSetMismatch,
    /// One file's received bytes do not hash to what the manifest claimed.
    FileHashMismatch {
        /// The file.
        path: String,
    },
    /// The number of op records received does not match `BundleManifest.op_count`.
    OpCountMismatch {
        /// What the manifest claimed.
        claimed: u64,
        /// What the stream actually carried.
        received: u64,
    },
    /// An op's `hlc.device` is not the manifest's own `device_id`.
    ForeignDevice,
    /// An op's signature did not verify against the manifest's `device_signing_public_key`.
    SignatureInvalid,
    /// Writing a file failed.
    Write(WriteError),
    /// The store failed.
    Store(txtodo_store::StoreError),
}

impl std::fmt::Display for BundleImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleImportError::Truncated => write!(f, "bundle ends before a whole frame"),
            BundleImportError::Header(e) => write!(f, "header: {e}"),
            BundleImportError::ManifestDecode(e) => write!(f, "manifest: {e}"),
            BundleImportError::UnsupportedVersion(v) => {
                write!(f, "bundle version {v} is not supported")
            }
            BundleImportError::SchemaMismatch { found, supported } => write!(
                f,
                "bundle schema {found} does not match this workspace's {supported}"
            ),
            BundleImportError::Decrypt(e) => write!(f, "{e}"),
            BundleImportError::Record(e) => write!(f, "{e}"),
            BundleImportError::BadPath(p) => write!(f, "bad path in bundle: {p:?}"),
            BundleImportError::FileSetMismatch => {
                write!(f, "the files received do not match the manifest")
            }
            BundleImportError::FileHashMismatch { path } => {
                write!(f, "{path}: bytes do not match the manifest's blake3 hash")
            }
            BundleImportError::OpCountMismatch { claimed, received } => write!(
                f,
                "manifest claimed {claimed} ops, the stream carried {received}"
            ),
            BundleImportError::ForeignDevice => {
                write!(f, "an op's device does not match the manifest's device_id")
            }
            BundleImportError::SignatureInvalid => write!(f, "an op's signature did not verify"),
            BundleImportError::Write(e) => write!(f, "{e}"),
            BundleImportError::Store(e) => write!(f, "store: {e}"),
        }
    }
}

impl std::error::Error for BundleImportError {}
