//! File-carrier acceptance tests (plan M8 `sync-file-carrier`, design §4.5): two carriers over one
//! shared directory converge with no network, a repeat scan or a truncated trailing frame never
//! desyncs a reader, a device can never write another device's file, and rotation plus a
//! multi-file scan still delivers every frame in order.

use std::fs::{self, OpenOptions};
use std::io::Write;

use tempfile::tempdir;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};

use crate::aead::GroupKey;
use crate::append_frame::AppendFrame;
use crate::carrier::{FileCarrier, parse_ops_file_name};
use crate::carrier_error::CarrierError;
use crate::frame::Frame;
use crate::link::Link;
use crate::message::{GroupId, OriginRange};
use crate::sealed_ops::{SealContext, seal_ops};
use crate::sign::DeviceSigningKey;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn frame(payload: u32) -> Frame {
    Frame::new(payload.to_le_bytes().to_vec()).unwrap()
}

#[test]
fn two_carriers_over_one_shared_directory_converge_with_no_network() {
    let dir = tempdir().unwrap();
    let device_a = dev(1);
    let device_b = dev(2);
    let mut carrier_a = FileCarrier::open(dir.path(), device_a).unwrap();
    let mut carrier_b = FileCarrier::open(dir.path(), device_b).unwrap();

    carrier_a.send(frame(1)).unwrap();
    assert_eq!(carrier_b.recv().unwrap(), frame(1));
    carrier_b.send(frame(2)).unwrap();
    assert_eq!(carrier_a.recv().unwrap(), frame(2));

    // Each device wrote only its own file — no manifest, no shared file, no conflict possible.
    let mut names: Vec<String> = fs::read_dir(dir.path().join("sync"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![format!("{device_a}.ops"), format!("{device_b}.ops")]
    );
}

#[test]
fn rereading_imports_nothing_twice_and_a_truncated_trailing_frame_completes_later() {
    let dir = tempdir().unwrap();
    let device_a = dev(1);
    let device_b = dev(2);
    let mut carrier_a = FileCarrier::open(dir.path(), device_a).unwrap();
    let mut carrier_b = FileCarrier::open(dir.path(), device_b).unwrap();

    carrier_a.send(frame(1)).unwrap();
    assert_eq!(carrier_b.poll().unwrap(), Some(frame(1)));
    // Re-scanning the same file, nothing new having landed, returns nothing — not frame(1) again.
    assert_eq!(carrier_b.poll().unwrap(), None);
    assert_eq!(carrier_b.poll().unwrap(), None);

    // Simulate a file-sync tool caught mid-copy: append only the first half of a second frame's
    // bytes directly (bypassing `FileCarrier`, which never writes a partial record itself).
    let record = AppendFrame::new(frame(2).encode().unwrap())
        .unwrap()
        .encode();
    let split = record.len() / 2;
    let path = dir.path().join("sync").join(format!("{device_a}.ops"));
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&record[..split]).unwrap();
    drop(file);

    // The trailing frame is incomplete: skipped, not an error, and not misread as something else.
    assert_eq!(carrier_b.poll().unwrap(), None);

    // The rest of the same record lands (the "sync" finishes copying).
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&record[split..]).unwrap();
    drop(file);

    // Now it completes, and completes to exactly frame(2) — no bytes lost or duplicated.
    assert_eq!(carrier_b.poll().unwrap(), Some(frame(2)));
    assert_eq!(carrier_b.poll().unwrap(), None);
}

#[test]
fn a_devices_write_to_another_devices_file_is_refused() {
    let dir = tempdir().unwrap();
    let device_a = dev(1);
    let device_b = dev(2);
    let carrier_a = FileCarrier::open(dir.path(), device_a).unwrap();

    let foreign_path = dir.path().join("sync").join(format!("{device_b}.ops"));
    let record = AppendFrame::new(frame(1).encode().unwrap())
        .unwrap()
        .encode();

    let err = carrier_a
        .write_frame_to(&foreign_path, &record)
        .unwrap_err();
    match err {
        CarrierError::ForeignDevice { path, owner, us } => {
            assert_eq!(path, foreign_path);
            assert_eq!(owner, device_b);
            assert_eq!(us, device_a);
        }
        other => panic!("expected ForeignDevice, got {other:?}"),
    }
    // Refused means refused: no file was created for device B at all.
    assert!(!foreign_path.exists());
}

#[test]
fn rotation_and_a_multi_file_scan_deliver_every_frame_in_order() {
    let dir = tempdir().unwrap();
    let device_a = dev(1);
    let device_b = dev(2);
    let mut carrier_a = FileCarrier::open(dir.path(), device_a).unwrap();
    let mut carrier_b = FileCarrier::open(dir.path(), device_b).unwrap();

    // Frames large enough that a handful of them cross MAX_OPS_FILE_BYTES twice, forcing rotation
    // to `<device>-1.ops` and then `<device>-2.ops` while writing this device's own stream.
    const FRAME_BYTES: usize = 1_000_000;
    const FRAMES: u32 = 20; // 20 MB total, well past two 8 MB rotations
    for i in 0..FRAMES {
        let mut body = vec![0u8; FRAME_BYTES];
        body[..4].copy_from_slice(&i.to_le_bytes());
        carrier_a.send(Frame::new(body).unwrap()).unwrap();
    }

    let mut rotated_files: Vec<String> = fs::read_dir(dir.path().join("sync"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_str().unwrap().to_string())
        .filter(|name| name.starts_with(&format!("{device_a}-")))
        .collect();
    rotated_files.sort();
    assert!(
        rotated_files.len() >= 2,
        "expected at least two rotated files, found {rotated_files:?}"
    );

    let mut received = Vec::new();
    while let Some(f) = carrier_b.poll().unwrap() {
        received.push(u32::from_le_bytes(f.body[..4].try_into().unwrap()));
    }
    assert_eq!(received, (0..FRAMES).collect::<Vec<_>>());
}

/// A real `Op` with a real group key, sealed exactly the way `lan_session.rs`/`bundle_export.rs`
/// seal a batch before it ever reaches a `Link`. Reused by both security tests below so the
/// "real ciphertext" and "real secret bytes" fixtures stay in one place.
fn sealed_frame_with_plaintext(device: DeviceId, plaintext_line: &str, key: &GroupKey) -> Frame {
    let op = Op {
        id: OpId::new(Ulid::from_u128(0xAB)),
        hlc: Hlc {
            wall_ms: 1_700_000_000_000,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new("todo.txt").unwrap(),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0xCD)),
            after: None,
            line: plaintext_line.to_owned(),
        },
    };
    let signing_key = DeviceSigningKey::from_bytes([0x11; 32]);
    let range = OriginRange {
        device,
        first: 1,
        last: 1,
    };
    let ctx = SealContext {
        group: GroupId(7),
        epoch: 0,
        key,
    };
    seal_ops(vec![op], vec![range], &signing_key, &ctx).unwrap_or_else(|e| panic!("seal_ops: {e}"))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

// security-m8-review gap 2 (checklist item 3, file-carrier half): the `.ops` file this carrier
// writes must hold ciphertext only. Encryption happens above the `Link` layer `FileCarrier`
// implements (same boundary as the relay's opaque blob store) — this proves that by construction,
// not by inspection: a real op with a distinctive plaintext description, sealed under a real
// group key, must leave neither the plaintext text nor the raw key bytes anywhere in the bytes
// this carrier actually put on disk.
#[test]
fn ops_on_disk_are_ciphertext_only_never_plaintext_or_the_group_key() {
    let dir = tempdir().unwrap();
    let device_a = dev(1);
    let mut carrier = FileCarrier::open(dir.path(), device_a).unwrap();

    let plaintext_line = "DEFINITELY-PLAINTEXT-buy-plutonium-for-the-reactor";
    let group_key_bytes = [0x5Bu8; 32];
    let group_key = GroupKey::from_bytes(group_key_bytes);

    let frame = sealed_frame_with_plaintext(device_a, plaintext_line, &group_key);
    carrier.send(frame).unwrap();

    let path = dir.path().join("sync").join(format!("{device_a}.ops"));
    let raw = fs::read(&path).unwrap();

    // Sanity: something real landed on disk (a vacuous pass on an empty file would prove nothing).
    assert!(
        raw.len() > 64,
        "expected real ciphertext on disk, got only {} bytes",
        raw.len()
    );
    assert!(
        !contains_bytes(&raw, plaintext_line.as_bytes()),
        "the plaintext task description leaked into the on-disk .ops file"
    );
    assert!(
        !contains_bytes(&raw, &group_key_bytes),
        "the raw group key bytes leaked into the on-disk .ops file"
    );
}

// security-m8-review gap 3 (checklist item 6): `--sync-dir` is external input — a shared folder
// this device does not control (Syncthing/Dropbox/iCloud Drive). A malicious peer sharing that
// folder could plant a symlink named exactly like a valid other-device `.ops` file, pointing at an
// arbitrary local path outside the sync directory. `read_dir` in `carrier.rs` now skips any
// directory entry that is itself a symlink (`std::fs::symlink_metadata`, which does not follow the
// link, unlike the default `metadata()`), so neither `other_device_files` nor `highest_rotation`
// ever hands such a name back. This proves the carrier never reads through one: the symlink's
// target holds a *validly framed* frame carrying a sentinel, so if the carrier ever followed the
// link, `poll` would return `Some` with the sentinel inside — the only way this test can fail is
// if the fix in `carrier.rs::read_dir` regresses.
#[cfg(unix)]
#[test]
fn a_symlink_planted_by_a_malicious_peer_in_sync_dir_is_never_followed() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let device_a = dev(1);
    let device_b = dev(2); // the device name the attacker's symlink impersonates
    let mut carrier_a = FileCarrier::open(dir.path(), device_a).unwrap();

    // A file genuinely outside the sync directory: a validly framed append-frame carrying a
    // sentinel. If `FileCarrier` ever opened this file, `poll` would decode it to exactly this
    // frame — so the sentinel surfacing at all, in any form, is the leak this test watches for.
    let outside = tempdir().unwrap();
    let secret_path = outside.path().join("id_rsa");
    let sentinel =
        Frame::new(b"SENTINEL: lives outside sync_dir, never legitimately readable here".to_vec())
            .unwrap();
    let record = AppendFrame::new(sentinel.encode().unwrap())
        .unwrap()
        .encode();
    fs::write(&secret_path, &record).unwrap();

    // The malicious symlink: named like a real other-device `.ops` file, pointing outside
    // `sync_dir` entirely.
    let link_path = dir.path().join("sync").join(format!("{device_b}.ops"));
    symlink(&secret_path, &link_path).unwrap();

    // Never followed, on any number of scans.
    assert_eq!(carrier_a.poll().unwrap(), None);
    assert_eq!(carrier_a.poll().unwrap(), None);
}

// security-m8-review gap 3, second half: `parse_ops_file_name`'s ULID parsing should already
// refuse any `..`- or `/`-bearing candidate name (a `/` cannot occur in one path component to
// begin with — the OS itself refuses to create such a name — but a component that is exactly
// `..`, or contains it as text, must still fail to parse as a ULID). This test only proves the
// existing behaviour; it changes nothing if it already passes, per the task's own instruction not
// to touch working code.
#[test]
fn parse_ops_file_name_rejects_dot_dot_and_slash_bearing_candidates() {
    for candidate in [
        "../../../etc/passwd.ops",
        "..ops",
        "..-1.ops",
        "a/b.ops",
        "/etc/passwd.ops",
        "...ops",
        "..",
    ] {
        assert!(
            parse_ops_file_name(candidate).is_none(),
            "{candidate:?} must not parse as a valid <device-id>[-<n>].ops name"
        );
    }
}
