//! Encrypted-file `KeyStore`: the `key_store = "file"` backend, and `"auto"`'s explicit fallback
//! once a human has chosen it ([`crate::keystore_resolve`]). One small file holding every secret,
//! re-sealed on every write. Refs: <https://docs.rs/argon2> · RFC 9106
//! <https://www.rfc-editor.org/rfc/rfc9106> · <https://docs.rs/chacha20poly1305>.
//!
//! Layout: `magic(4) || version(u16) || memory_kib(u32) || iterations(u32) || parallelism(u32) ||
//! salt(16) || nonce(24) || ciphertext`. Everything up to the nonce is the clear header and is also
//! the AEAD associated data, so a header field cannot be swapped without failing the tag check.
//! Argon2 parameters travel in the header (not a fixed constant) precisely so raising the constants
//! later does not strand a file written under the old ones.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::keystore::{KeyId, KeyStore, Secret};
use crate::keystore_error::KeyStoreError;

const MAGIC: &[u8; 4] = b"TXKS";
const FORMAT_VERSION: u16 = 1;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const KEY_BYTES: usize = 32;
/// Header length up to and including the salt; this is also the AEAD associated data.
const HEADER_BYTES: usize = 4 + 2 + 4 + 4 + 4 + SALT_BYTES;

/// Argon2id memory cost for a freshly created file, in KiB (~19 MiB; OWASP-recommended floor).
pub const ARGON2_MEMORY_KIB: u32 = 19_456;
/// Argon2id iteration count for a freshly created file.
pub const ARGON2_ITERATIONS: u32 = 2;
/// Argon2id parallelism (lanes) for a freshly created file.
pub const ARGON2_PARALLELISM: u32 = 1;

/// One entry per `KeyId`, sealed as a whole. Ordinary maps only — no growth without bound, since
/// there is one entry per key the device holds and that set is itself capped elsewhere
/// ([`crate::keystore::MAX_STORED_EPOCHS`]).
type EntryMap = BTreeMap<KeyId, Vec<u8>>;

/// The encrypted-file backend. Holds the derived key, never the passphrase, past `open`/`create`.
pub struct FileKeyStore {
    path: PathBuf,
    header: [u8; HEADER_BYTES],
    key: [u8; KEY_BYTES],
}

impl Drop for FileKeyStore {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.key);
    }
}

impl std::fmt::Debug for FileKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileKeyStore")
            .field("path", &self.path)
            .field("key", &"<redacted>")
            .finish()
    }
}

fn io_err(path: &Path, e: std::io::Error) -> KeyStoreError {
    KeyStoreError::Io {
        path: path.to_path_buf(),
        reason: e.to_string(),
    }
}

fn derive_key(
    passphrase: &Secret,
    salt: &[u8; SALT_BYTES],
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> Result<[u8; KEY_BYTES], KeyStoreError> {
    let params =
        Params::new(memory_kib, iterations, parallelism, Some(KEY_BYTES)).map_err(|e| {
            KeyStoreError::Corrupt {
                path: PathBuf::new(),
                reason: format!("invalid Argon2 parameters: {e}"),
            }
        })?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_BYTES];
    argon2
        .hash_password_into(passphrase.expose(), salt, &mut key)
        .map_err(|e| KeyStoreError::Corrupt {
            path: PathBuf::new(),
            reason: format!("Argon2 key derivation failed: {e}"),
        })?;
    Ok(key)
}

fn header_bytes(
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: &[u8; SALT_BYTES],
) -> [u8; HEADER_BYTES] {
    let mut header = [0u8; HEADER_BYTES];
    header[0..4].copy_from_slice(MAGIC);
    header[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[6..10].copy_from_slice(&memory_kib.to_le_bytes());
    header[10..14].copy_from_slice(&iterations.to_le_bytes());
    header[14..18].copy_from_slice(&parallelism.to_le_bytes());
    header[18..18 + SALT_BYTES].copy_from_slice(salt);
    debug_assert_eq!(18 + SALT_BYTES, HEADER_BYTES);
    header
}

#[cfg(unix)]
fn check_permissions(path: &Path) -> Result<(), KeyStoreError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(path).map_err(|e| io_err(path, e))?;
    let mode = meta.permissions().mode() & 0o777;
    // Refused, never repaired (task notes): a permissive file may already have been read.
    if mode & 0o077 != 0 {
        return Err(KeyStoreError::PermissionsTooOpen {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path) -> Result<(), KeyStoreError> {
    Ok(())
}

fn random_bytes<const N: usize>() -> Result<[u8; N], KeyStoreError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|_| KeyStoreError::Entropy)?;
    Ok(buf)
}

impl FileKeyStore {
    /// Creates a new, empty keystore file at `path`, refusing to overwrite an existing one. Written
    /// with mode `0600` from the start on Unix, never `chmod`'d after the fact.
    pub fn create(path: &Path, passphrase: &Secret) -> Result<FileKeyStore, KeyStoreError> {
        if path.exists() {
            return Err(KeyStoreError::Io {
                path: path.to_path_buf(),
                reason: "refusing to overwrite an existing keystore file".to_string(),
            });
        }
        let salt = random_bytes::<SALT_BYTES>()?;
        let key = derive_key(
            passphrase,
            &salt,
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
        )?;
        let header = header_bytes(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            &salt,
        );
        let store = FileKeyStore {
            path: path.to_path_buf(),
            header,
            key,
        };
        store.write_entries(&EntryMap::new(), true)?;
        Ok(store)
    }

    /// Opens an existing keystore file, deriving the key from its own stored Argon2 parameters so a
    /// file written under older, lower constants still opens after the constants are raised.
    pub fn open(path: &Path, passphrase: &Secret) -> Result<FileKeyStore, KeyStoreError> {
        check_permissions(path)?;
        let bytes = fs::read(path).map_err(|e| io_err(path, e))?;
        if bytes.len() < HEADER_BYTES + NONCE_BYTES {
            return Err(KeyStoreError::Corrupt {
                path: path.to_path_buf(),
                reason: "file shorter than the header".to_string(),
            });
        }
        if &bytes[0..4] != MAGIC {
            return Err(KeyStoreError::Corrupt {
                path: path.to_path_buf(),
                reason: "bad magic".to_string(),
            });
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != FORMAT_VERSION {
            return Err(KeyStoreError::Corrupt {
                path: path.to_path_buf(),
                reason: format!("unsupported format version {version}"),
            });
        }
        let memory_kib = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        let iterations = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let parallelism = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
        let mut salt = [0u8; SALT_BYTES];
        salt.copy_from_slice(&bytes[18..18 + SALT_BYTES]);
        let header = header_bytes(memory_kib, iterations, parallelism, &salt);
        debug_assert_eq!(&header[..], &bytes[..HEADER_BYTES]);
        let key = derive_key(passphrase, &salt, memory_kib, iterations, parallelism)?;
        let store = FileKeyStore {
            path: path.to_path_buf(),
            header,
            key,
        };
        // Verifies the passphrase up front rather than on the first `get`.
        store.read_entries(&bytes)?;
        Ok(store)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.key).into())
    }

    /// Decrypts and decodes the entry map from a full file's bytes, previously read by the caller so
    /// `open` can verify the passphrase without a second disk read.
    fn read_entries(&self, file_bytes: &[u8]) -> Result<EntryMap, KeyStoreError> {
        let nonce = &file_bytes[HEADER_BYTES..HEADER_BYTES + NONCE_BYTES];
        let ciphertext = &file_bytes[HEADER_BYTES + NONCE_BYTES..];
        let plaintext = self
            .cipher()
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &self.header,
                },
            )
            .map_err(|_| KeyStoreError::WrongPassphrase {
                path: self.path.clone(),
            })?;
        postcard::from_bytes(&plaintext).map_err(|e| KeyStoreError::Corrupt {
            path: self.path.clone(),
            reason: format!("entry map did not decode: {e}"),
        })
    }

    fn load(&self) -> Result<EntryMap, KeyStoreError> {
        check_permissions(&self.path)?;
        let bytes = fs::read(&self.path).map_err(|e| io_err(&self.path, e))?;
        self.read_entries(&bytes)
    }

    /// Seals `entries` and writes the file atomically (temp file at mode `0600`, then rename).
    /// `first_write` skips the permission check, since the file does not exist yet.
    fn write_entries(&self, entries: &EntryMap, first_write: bool) -> Result<(), KeyStoreError> {
        if !first_write {
            check_permissions(&self.path)?;
        }
        let plaintext =
            postcard::to_allocvec(entries).map_err(|e| KeyStoreError::Encode(e.to_string()))?;
        let nonce = random_bytes::<NONCE_BYTES>()?;
        let ciphertext = self
            .cipher()
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &self.header,
                },
            )
            .map_err(|_| KeyStoreError::Corrupt {
                path: self.path.clone(),
                reason: "AEAD seal failed".to_string(),
            })?;
        let mut out = Vec::with_capacity(HEADER_BYTES + NONCE_BYTES + ciphertext.len());
        out.extend_from_slice(&self.header);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        debug_assert!(out.len() > HEADER_BYTES + NONCE_BYTES);

        let tmp_path = self.path.with_extension("tmp");
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        {
            use std::io::Write;
            let mut f = opts.open(&tmp_path).map_err(|e| io_err(&tmp_path, e))?;
            f.write_all(&out).map_err(|e| io_err(&tmp_path, e))?;
            f.sync_all().map_err(|e| io_err(&tmp_path, e))?;
        }
        if first_write {
            // `hard_link` fails atomically if `self.path` already exists (unlike `rename`, which
            // would silently replace it) — closes the TOCTOU window after the `exists()` check in
            // `create`.
            let linked = fs::hard_link(&tmp_path, &self.path);
            let _ = fs::remove_file(&tmp_path);
            linked.map_err(|e| io_err(&self.path, e))?;
        } else {
            fs::rename(&tmp_path, &self.path).map_err(|e| io_err(&self.path, e))?;
        }
        debug_assert!(self.path.exists());
        Ok(())
    }
}

impl KeyStore for FileKeyStore {
    fn get(&self, id: KeyId) -> Result<Option<Secret>, KeyStoreError> {
        let entries = self.load()?;
        Ok(entries.get(&id).cloned().map(Secret::new))
    }

    fn put(&self, id: KeyId, secret: &Secret) -> Result<(), KeyStoreError> {
        let mut entries = self.load()?;
        entries.insert(id, secret.expose().to_vec());
        self.write_entries(&entries, false)
    }

    fn delete(&self, id: KeyId) -> Result<(), KeyStoreError> {
        let mut entries = self.load()?;
        entries.remove(&id);
        self.write_entries(&entries, false)
    }
}
