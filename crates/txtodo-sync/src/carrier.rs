//! The file carrier (plan M8 `sync-file-carrier`, design §4.5): a [`Link`] backed by a shared
//! folder instead of a socket. Own-file-only is the whole trick — each device appends only to
//! `sync/<device-id>[-<n>].ops`, so dumb file sync (Syncthing / Dropbox / iCloud Drive) never sees
//! two devices touch the same file and therefore never has a conflict to resolve. `recv` polls the
//! folder for every *other* device's files.
//!
//! This module never touches `txtodo-store`: it moves [`Frame`]s, exactly like [`crate::ChannelLink`]
//! and [`crate::IrohLink`]. Importing decoded ops into the store and deduping against
//! `ops.op_id UNIQUE` is a different layer's job (the eventual daemon-level sync engine) — seam
//! deliberately not built here. What *is* built here is this module's own idempotence: a tracked
//! per-file byte offset means a repeat scan never re-emits a frame this carrier already returned.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use txtodo_model::{DeviceId, Ulid};

use crate::append_frame::AppendFrame;
use crate::carrier_error::CarrierError;
use crate::frame::Frame;
use crate::link::{Link, LinkError};

/// Rotate the current write file once it reaches this size. Keeps any one `.ops` file from growing
/// without bound the same way every other unbounded collection in this crate has a named cap.
pub const MAX_OPS_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// How long [`Link::recv`]'s blocking poll loop sleeps between directory scans when nothing new is
/// there yet. A file carrier has no socket to block on, so this is the closest equivalent to
/// [`crate::IrohLink`]'s `IDLE_TIMEOUT` — short enough that a real sync loop still feels responsive,
/// long enough not to busy-spin the disk.
const RECV_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A `.ops` file name, parsed: which device it belongs to and its rotation index (`0` = the
/// unsuffixed base file, matching [`ops_file_name`]). `pub(crate)` only because it is
/// [`parse_ops_file_name`]'s return type, which `carrier_tests.rs` needs to call; its fields stay
/// private, so it carries no API promise beyond "parsing succeeded or it didn't".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OpsFileName {
    device: DeviceId,
    rotation: u32,
}

/// The file name for `device`'s `rotation`-th ops file: `<device-id>.ops` for `0`, else
/// `<device-id>-<rotation>.ops`.
fn ops_file_name(device: DeviceId, rotation: u32) -> String {
    if rotation == 0 {
        format!("{device}.ops")
    } else {
        format!("{device}-{rotation}.ops")
    }
}

/// Parses a file name as `<device-id>[-<n>].ops`. `None` for anything else — a foreign program's
/// file in the same folder, a temp file a sync client left behind, and so on; never a panic on
/// unrecognised input, since every name in this directory that we did not just write is external.
/// `pub(crate)` rather than private so `carrier_tests.rs` can assert directly that a `..`- or
/// `/`-bearing candidate name is already rejected by the `Ulid::parse` call below, without a
/// public API promise beyond `FileCarrier` itself (same reasoning as `write_frame_to`).
pub(crate) fn parse_ops_file_name(name: &str) -> Option<OpsFileName> {
    let stem = name.strip_suffix(".ops")?;
    match stem.split_once('-') {
        // A ULID's own text never contains `-` (Crockford base32), so the first `-` is always the
        // rotation separator, never part of the device id.
        Some((device_str, rotation_str)) => {
            let device = DeviceId::new(Ulid::parse(device_str)?);
            let rotation: u32 = rotation_str.parse().ok()?;
            if rotation == 0 {
                return None; // "-0" is not how the base file is spelled
            }
            Some(OpsFileName { device, rotation })
        }
        None => Some(OpsFileName {
            device: DeviceId::new(Ulid::parse(stem)?),
            rotation: 0,
        }),
    }
}

fn io_err(path: &Path, e: std::io::Error) -> CarrierError {
    CarrierError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}

/// A [`Link`] over a shared folder. `send` appends to this device's own file; `recv` scans every
/// other device's files. See the module doc for why op-level dedup is out of scope here.
pub struct FileCarrier {
    /// `<configured dir>/sync` — the subdirectory the `.ops` files actually live in.
    sync_dir: PathBuf,
    device: DeviceId,
    /// Rotation index of the file we are currently appending to.
    write_rotation: u32,
    /// Bytes already returned from each other-device file, keyed by its path. In-memory only —
    /// this carrier's own idempotence covers repeat scans within one process lifetime; it is not a
    /// substitute for `ops.op_id` dedup at whatever layer eventually imports these frames into a
    /// store across restarts.
    offsets: BTreeMap<PathBuf, u64>,
}

impl FileCarrier {
    /// Opens a file carrier rooted at `dir` for `device`, creating `<dir>/sync` if it does not
    /// exist yet. Resumes appending after this device's own highest existing rotation file, so a
    /// restart never starts writing `sync/<device>.ops` from scratch over already-shared bytes.
    pub fn open(dir: impl AsRef<Path>, device: DeviceId) -> Result<FileCarrier, CarrierError> {
        let sync_dir = dir.as_ref().join("sync");
        std::fs::create_dir_all(&sync_dir).map_err(|e| CarrierError::Dir {
            path: sync_dir.clone(),
            message: e.to_string(),
        })?;
        let write_rotation = highest_rotation(&sync_dir, device)?;
        Ok(FileCarrier {
            sync_dir,
            device,
            write_rotation,
            offsets: BTreeMap::new(),
        })
    }

    fn write_path(&self) -> PathBuf {
        self.sync_dir
            .join(ops_file_name(self.device, self.write_rotation))
    }

    /// Appends one already-encoded [`Frame`] to this device's own file, rotating first if the
    /// current file has reached [`MAX_OPS_FILE_BYTES`].
    fn append(&mut self, frame: &Frame) -> Result<(), CarrierError> {
        let body = frame.encode().map_err(CarrierError::Frame)?;
        let record = AppendFrame::new(body)?.encode();
        let mut path = self.write_path();
        let needs_rotation = match std::fs::metadata(&path) {
            Ok(meta) => meta.len() >= MAX_OPS_FILE_BYTES,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io_err(&path, e)),
        };
        if needs_rotation {
            self.write_rotation += 1;
            path = self.write_path();
        }
        self.write_frame_to(&path, &record)
    }

    /// Appends already-encoded bytes to `path`, refusing before touching it if `path`'s own file
    /// name does not belong to `self.device` — design §4.5's "one device never writes another
    /// device's file" invariant, checked here rather than assumed. `append` is the only caller in
    /// this crate, and it always derives `path` from `self.device` itself, so this should never
    /// actually fire in correct code; it is kept as a checked `Result` (never a `debug_assert!`)
    /// so a construction bug is a typed refusal, never a silent write into a peer's file — see
    /// `carrier_tests.rs`'s `write_to_another_devices_file_is_refused` for exactly this path.
    /// `pub(crate)` rather than private so that test can reach it directly, without a public API
    /// promise beyond `Link::send`.
    pub(crate) fn write_frame_to(&self, path: &Path, bytes: &[u8]) -> Result<(), CarrierError> {
        let name =
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| CarrierError::BadFileName {
                    path: path.to_path_buf(),
                })?;
        let parsed = parse_ops_file_name(name).ok_or_else(|| CarrierError::BadFileName {
            path: path.to_path_buf(),
        })?;
        if parsed.device != self.device {
            return Err(CarrierError::ForeignDevice {
                path: path.to_path_buf(),
                owner: parsed.device,
                us: self.device,
            });
        }
        // https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.append — append() alone
        // is what makes concurrent readers safe to observe a growing file without ever seeing a
        // rewrite of bytes they already read.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| io_err(path, e))?;
        file.write_all(bytes).map_err(|e| io_err(path, e))?;
        file.flush().map_err(|e| io_err(path, e))
    }

    /// Scans other devices' files for the next frame this carrier has not already returned.
    /// `Ok(None)` when there is nothing new right now — including a truncated trailing frame,
    /// which is left in place for a later scan once the rest of it has landed on disk (see
    /// [`AppendFrame::try_decode`]). Non-blocking, unlike [`Link::recv`] (which loops this with a
    /// sleep between empty scans) — a caller with its own event loop can call this directly rather
    /// than dedicate a thread to a blocking `recv`.
    pub fn poll(&mut self) -> Result<Option<Frame>, CarrierError> {
        for path in other_device_files(&self.sync_dir, self.device)? {
            let offset = *self.offsets.get(&path).unwrap_or(&0);
            let tail = read_tail(&path, offset)?;
            let Some((record, used)) = AppendFrame::try_decode(&tail)? else {
                continue; // no complete frame at this file's current offset yet
            };
            self.offsets.insert(path, offset + used as u64);
            let (frame, consumed) = Frame::decode(&record.body).map_err(CarrierError::Frame)?;
            debug_assert_eq!(
                consumed,
                record.body.len(),
                "one frame fills one append-frame body"
            );
            return Ok(Some(frame));
        }
        Ok(None)
    }
}

impl Link for FileCarrier {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        self.append(&frame).map_err(LinkError::from)
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        loop {
            match self.poll() {
                Ok(Some(frame)) => return Ok(frame),
                Ok(None) => std::thread::sleep(RECV_POLL_INTERVAL),
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// The highest rotation index already on disk for `device`, or `0` if it has never written here.
fn highest_rotation(sync_dir: &Path, device: DeviceId) -> Result<u32, CarrierError> {
    let mut highest = 0u32;
    for entry in read_dir(sync_dir)? {
        if let Some(parsed) = parse_ops_file_name(&entry)
            && parsed.device == device
            && parsed.rotation > highest
        {
            highest = parsed.rotation;
        }
    }
    Ok(highest)
}

/// Every other device's `.ops` file under `sync_dir`, sorted by parsed `(device, rotation)` —
/// never by raw file-name text, which would put `-10` before `-2`. This is what "readers scan all
/// matching files in filename order" (notes.md) means: file-creation order, derived from the name,
/// not a lexical string sort.
fn other_device_files(sync_dir: &Path, us: DeviceId) -> Result<Vec<PathBuf>, CarrierError> {
    let mut found: Vec<(OpsFileName, PathBuf)> = Vec::new();
    for name in read_dir(sync_dir)? {
        if let Some(parsed) = parse_ops_file_name(&name)
            && parsed.device != us
        {
            found.push((parsed, sync_dir.join(&name)));
        }
    }
    found.sort_by_key(|(parsed, _)| *parsed);
    Ok(found.into_iter().map(|(_, path)| path).collect())
}

/// Every plain-file name directly under `dir` — never a symlink's name, even one that would
/// otherwise parse as a valid `<device-id>[-<n>].ops` file. This directory is an untrusted shared
/// folder by design (Syncthing / Dropbox / iCloud Drive — see `tasks/sync-file-carrier/notes.md`),
/// so a malicious peer sharing it could plant a symlink named like a real other-device `.ops` file
/// pointing at an arbitrary local path (e.g. `~/.ssh/id_rsa`). `std::fs::read_dir`'s `DirEntry`
/// does not itself follow a symlink, but nothing downstream of this function did either:
/// `other_device_files`/`highest_rotation` would have handed the name straight back, and
/// `read_tail` opens the resulting path with `std::fs::File::open`, which *does* follow it.
/// `std::fs::symlink_metadata` (never `entry.metadata()`/`std::fs::metadata`, both of which follow
/// the link) is the only way to see "this name is a symlink" before that read happens.
/// <https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html>
///
/// A symlink is skipped, not a hard `CarrierError`: it is not necessarily hostile (a user's own
/// tooling could plant one in the sync folder), this directory is rescanned on every poll, and a
/// hard failure here would let one symlink — malicious or not — wedge every future scan rather
/// than just being invisible to it. This is the same silent-skip stance `parse_ops_file_name`
/// already takes on any other name in this folder that isn't a recognized `.ops` file.
fn read_dir(dir: &Path) -> Result<Vec<String>, CarrierError> {
    let entries = std::fs::read_dir(dir).map_err(|e| CarrierError::Dir {
        path: dir.to_path_buf(),
        message: e.to_string(),
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| CarrierError::Dir {
            path: dir.to_path_buf(),
            message: e.to_string(),
        })?;
        let path = dir.join(entry.file_name());
        // Gone between the readdir and this stat (a racing delete): treat it as "not a symlink we
        // need to skip" either way — the next step (opening it) will hit its own real error if it
        // still matters.
        let is_symlink = std::fs::symlink_metadata(&path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Reads `path` from `offset` to end-of-file. `path` not existing yet (a device advertised through
/// no file at all) is not modelled here — `other_device_files` only ever returns files that already
/// exist, so a `NotFound` here is a real error, not a race to paper over.
fn read_tail(path: &Path, offset: u64) -> Result<Vec<u8>, CarrierError> {
    let mut file = std::fs::File::open(path).map_err(|e| io_err(path, e))?;
    // https://doc.rust-lang.org/std/io/trait.Seek.html — seeking past a growing file's current
    // length is well-defined (a later read just returns fewer bytes), which matters here because
    // another process may be appending concurrently.
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| io_err(path, e))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| io_err(path, e))?;
    Ok(buf)
}
