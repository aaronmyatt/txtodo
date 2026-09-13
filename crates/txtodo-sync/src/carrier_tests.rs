//! File-carrier acceptance tests (plan M8 `sync-file-carrier`, design §4.5): two carriers over one
//! shared directory converge with no network, a repeat scan or a truncated trailing frame never
//! desyncs a reader, a device can never write another device's file, and rotation plus a
//! multi-file scan still delivers every frame in order.

use std::fs::{self, OpenOptions};
use std::io::Write;

use tempfile::tempdir;
use txtodo_model::{DeviceId, Ulid};

use crate::append_frame::AppendFrame;
use crate::carrier::FileCarrier;
use crate::carrier_error::CarrierError;
use crate::frame::Frame;
use crate::link::Link;

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
