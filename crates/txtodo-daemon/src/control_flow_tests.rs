//! The control channel across a rotation, two real `drive_control_session`s over a real channel
//! link (task `peer-prune`, ADR 0044): a device behind is brought up by the grants, a dropped one
//! gets nothing, and either side may be the one that dialed.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use txtodo_model::DeviceId;
use txtodo_store::NewDevice;
use txtodo_sync::{ChannelLink, Frame, GroupId, KeyId, Link, LinkError, Secret, channel_link_pair};

use crate::clock::FakeClock;
use crate::control_session::drive_control_session;
use crate::device_identity::DeviceIdentity;
use crate::group_rotation::believed_epoch;
use crate::workspace_registry::WorkspaceRegistry;

/// A `ChannelLink` that reads as closed after `idle` of silence, as an `IrohLink` does: with two
/// real drivers on one pair neither would ever stop reading.
struct IdleLink {
    inner: ChannelLink,
    idle: Duration,
}

impl Link for IdleLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        self.inner.send(frame)
    }
    fn recv(&mut self) -> Result<Frame, LinkError> {
        self.inner.recv_timeout(self.idle)?.ok_or(LinkError::Closed)
    }
}

struct Dev {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    id: Arc<DeviceIdentity>,
    registry: Arc<Mutex<WorkspaceRegistry>>,
}

/// Device `n`: its id comes from the clock (a lower `n` is a lower id), in group 42 with the
/// group's first key.
fn dev(n: u64) -> Dev {
    let dirs = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let id = DeviceIdentity::open_in_memory(dirs.0.path(), &FakeClock::new(1_000 * n)).unwrap();
    id.set_group(GroupId(42));
    id.key_store()
        .put(KeyId::Group(0), &Secret::new(vec![5; 32]))
        .unwrap();
    let registry = WorkspaceRegistry::open(&dirs.1.path().join("registry.db")).unwrap();
    Dev {
        _dirs: dirs,
        id: Arc::new(id),
        registry: Arc::new(Mutex::new(registry)),
    }
}

fn know(holder: &Dev, peer: &Dev) {
    let new = NewDevice {
        device: peer.id.device(),
        name: String::new(),
        static_public: peer.id.device_static_public().to_bytes(),
        paired_at_ms: 500,
        last_known_wall_ms: None,
        key_epoch: 0,
    };
    let mut store = holder.id.store().lock().unwrap();
    store.register_device_as(&new, true).unwrap();
}

/// Runs one session between `dialer` and `acceptor` to the end (each reads as closed after 400 ms
/// of silence). Both finish before this returns.
fn session(dialer: &Dev, acceptor: &Dev) {
    let (a, b) = channel_link_pair();
    let idle = Duration::from_millis(400);
    let run = |dev: &Dev, link: ChannelLink, dialed: Option<DeviceId>| {
        let (id, registry) = (Arc::clone(&dev.id), Arc::clone(&dev.registry));
        std::thread::spawn(move || {
            let mut link = IdleLink { inner: link, idle };
            drive_control_session(&mut link, &id, &registry, dialed)
        })
    };
    let dialing = run(dialer, a, Some(acceptor.id.device()));
    let accepting = run(acceptor, b, None);
    dialing.join().unwrap();
    accepting.join().unwrap();
}

fn key_at(dev: &Dev, epoch: u32) -> Option<Vec<u8>> {
    let held = dev.id.key_store().get(KeyId::Group(epoch)).unwrap();
    held.map(|s| s.expose().to_vec())
}

fn removed(dev: &Dev, peer: &Dev) -> bool {
    let row = dev.id.store().lock().unwrap().device(peer.id.device());
    row.unwrap().is_some_and(|r| r.removed_at_ms.is_some())
}

/// `a` (rotated to epoch 1, having dropped `c`) and `b` (still on epoch 0) know each other and `c`.
fn a_rotated_b_not() -> (Dev, Dev, Dev) {
    let (a, b, c) = (dev(1), dev(2), dev(3));
    for (x, y) in [(&a, &b), (&b, &a), (&a, &c), (&b, &c), (&c, &a)] {
        know(x, y);
    }
    a.id.remove_device(c.id.device(), 2_000).unwrap();
    assert_eq!(a.id.group_epoch(), 1);
    (a, b, c)
}

#[test]
fn a_device_behind_is_brought_up_when_the_one_ahead_dials() {
    let (a, b, c) = a_rotated_b_not();
    session(&a, &b);
    assert_eq!(b.id.group_epoch(), 1);
    assert_eq!(key_at(&b, 1), key_at(&a, 1));
    assert!(removed(&b, &c), "and drops whom the rotation dropped");
    assert_eq!(believed_epoch(&a.id, b.id.device()), Some(1), "B acked it");
}

#[test]
fn a_device_behind_is_brought_up_when_it_is_the_one_that_dials() {
    let (a, b, _c) = a_rotated_b_not();
    session(&b, &a);
    assert_eq!(b.id.group_epoch(), 1);
    assert_eq!(key_at(&b, 1), key_at(&a, 1));
    assert_eq!(believed_epoch(&a.id, b.id.device()), Some(1));
}

#[test]
fn a_dropped_device_gets_no_grant_and_stays_on_its_epoch() {
    let (a, _b, c) = a_rotated_b_not();
    session(&c, &a);
    assert_eq!(c.id.group_epoch(), 0, "no grant reached it");
    assert!(key_at(&c, 1).is_none());
    let session_again = || session(&a, &c);
    session_again();
    assert_eq!(c.id.group_epoch(), 0, "dialing it does not help either");
}

#[test]
fn a_device_two_epochs_behind_is_brought_up_through_both() {
    let (a, b, _c) = a_rotated_b_not();
    let d = dev(4);
    know(&a, &d);
    know(&b, &d);
    a.id.remove_device(d.id.device(), 2_500).unwrap();
    assert_eq!(a.id.group_epoch(), 2);
    session(&a, &b);
    assert_eq!(b.id.group_epoch(), 2);
    assert_eq!(key_at(&b, 2), key_at(&a, 2));
    assert_eq!(key_at(&b, 1), key_at(&a, 1), "and holds the one between");
}

#[test]
fn two_devices_on_the_same_epoch_still_exchange_what_they_always_did() {
    let (a, b) = (dev(1), dev(2));
    know(&a, &b);
    know(&b, &a);
    session(&a, &b);
    assert_eq!((a.id.group_epoch(), b.id.group_epoch()), (0, 0));
    assert_eq!(believed_epoch(&a.id, b.id.device()), Some(0));
}
