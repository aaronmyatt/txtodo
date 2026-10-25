//! The file carrier across a rotation (task `peer-prune`, ADR 0044): a frame is sealed under its
//! sender's current epoch and opened under any epoch the receiver retains, and its ops are verified
//! with the key of that epoch, not the receiver's current one.

use super::*;
use crate::group_epoch::EpochKey;
use crate::lan_session_tests::{one_peer_op, peer_device};
use txtodo_model::TaskId;
use txtodo_sync::{GroupKey, OriginRange, sign};

fn key(epoch: u32, byte: u8) -> EpochKey {
    EpochKey {
        epoch,
        key: GroupKey::from_bytes([byte; 32]),
    }
}

fn keys(held: &[&EpochKey]) -> GroupKeys {
    let mut keys = GroupKeys::new();
    for k in held {
        keys.insert(k.epoch, k.key.clone()).unwrap();
    }
    keys
}

/// One signed op, as `serve_want` builds it, signed with `signer`'s epoch key.
fn ops_message(workspace: WorkspaceId, signer: &EpochKey) -> Message {
    let op = one_peer_op(TaskId::new(txtodo_model::Ulid::from_u128(5)));
    let signature = sign(&op, &signer.signing_key()).unwrap();
    Message::Ops {
        workspace: workspace.ulid().to_u128(),
        ops: vec![op],
        signatures: vec![signature],
        ranges: vec![OriginRange {
            device: peer_device(),
            first: 1,
            last: 1,
        }],
    }
}

fn workspace() -> WorkspaceId {
    WorkspaceId::new(txtodo_model::Ulid::from_u128(77))
}

#[test]
fn a_frame_sealed_before_the_rotation_still_opens_and_verifies_after_it() {
    let (old, new) = (key(0, 1), key(1, 2));
    let group = GroupId(3);
    let frame = seal_message(group, workspace(), &old, &ops_message(workspace(), &old)).unwrap();
    // The receiver has rotated: its current epoch is 1, and it retains 0.
    let ops = open_and_verify(&frame, group, workspace(), &keys(&[&old, &new]));
    assert_eq!(ops.map(|o| o.len()), Some(1));
}

#[test]
fn a_frame_sealed_under_the_new_epoch_opens_and_verifies_with_the_new_key() {
    let (old, new) = (key(0, 1), key(1, 2));
    let group = GroupId(3);
    let frame = seal_message(group, workspace(), &new, &ops_message(workspace(), &new)).unwrap();
    assert_eq!(
        sealed_epoch(&frame.body),
        Some(1),
        "sealed under the sender's current epoch"
    );
    assert!(open_and_verify(&frame, group, workspace(), &keys(&[&old, &new])).is_some());
}

#[test]
fn ops_signed_with_another_epochs_key_are_refused() {
    let (old, new) = (key(0, 1), key(1, 2));
    let group = GroupId(3);
    // Sealed under epoch 1 but signed with epoch 0's key: the signature does not belong to its epoch.
    let frame = seal_message(group, workspace(), &new, &ops_message(workspace(), &old)).unwrap();
    assert!(open_and_verify(&frame, group, workspace(), &keys(&[&old, &new])).is_none());
}

#[test]
fn a_frame_from_an_epoch_the_receiver_does_not_hold_is_dropped() {
    let (old, new) = (key(0, 1), key(1, 2));
    let group = GroupId(3);
    let frame = seal_message(group, workspace(), &new, &ops_message(workspace(), &new)).unwrap();
    assert!(open_and_verify(&frame, group, workspace(), &keys(&[&old])).is_none());
}
