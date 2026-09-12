//! Chooses a backend for `config.toml`'s `key_store = "auto" | "os" | "file"`. The one rule that
//! matters (task notes): `auto` never writes a key file on its own. If the OS backend is not
//! reachable, resolution stops and returns an error telling the human what to decide — it does not
//! silently downgrade from an OS-protected store to a file in their home directory.

use crate::keystore::KeyStore;
use crate::keystore_error::KeyStoreError;

/// The three values `config.toml`'s `key_store` may take.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyStoreMode {
    /// Prefer the OS backend; stop and ask rather than fall back to a file.
    Auto,
    /// OS keystore only. Unavailable is a hard error naming the reason.
    Os,
    /// Encrypted file only, chosen deliberately.
    File,
}

/// Backend actually selected, named so `txtodo doctor` can print it without re-deriving anything.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResolvedBackend {
    /// The OS keystore (Keychain / Credential Manager / Secret Service).
    Os,
    /// The encrypted-file fallback.
    File,
}

impl ResolvedBackend {
    /// The name `txtodo doctor` prints.
    pub fn name(self) -> &'static str {
        match self {
            ResolvedBackend::Os => "os",
            ResolvedBackend::File => "file",
        }
    }
}

/// Picks a backend per `mode`, using `probe_os` to test OS-backend reachability without this crate
/// ever calling into the real `keyring` crate directly (tests inject a fake; the real caller passes
/// `|| OsKeyStore::probe(scope)`). `make_os` and `make_file` construct the chosen backend; `make_file`
/// is called only for `File`, or for `Auto`/`Os` — never invented on the caller's behalf.
pub fn resolve<Os: KeyStore + 'static, File: KeyStore + 'static>(
    mode: KeyStoreMode,
    probe_os: impl FnOnce() -> Result<(), String>,
    make_os: impl FnOnce() -> Os,
    make_file: impl FnOnce() -> Result<File, KeyStoreError>,
) -> Result<(ResolvedBackend, Box<dyn KeyStore>), KeyStoreError> {
    match mode {
        KeyStoreMode::Os => match probe_os() {
            Ok(()) => Ok((ResolvedBackend::Os, Box::new(make_os()))),
            Err(reason) => Err(KeyStoreError::Unavailable {
                backend: "os",
                reason,
            }),
        },
        KeyStoreMode::File => Ok((ResolvedBackend::File, Box::new(make_file()?))),
        KeyStoreMode::Auto => match probe_os() {
            Ok(()) => Ok((ResolvedBackend::Os, Box::new(make_os()))),
            // The one line this module exists to enforce: no file is created here.
            Err(reason) => Err(KeyStoreError::AutoNeedsChoice { reason }),
        },
    }
}
