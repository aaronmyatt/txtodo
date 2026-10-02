//! `SyncStatusResponse.protocol`, `Peer.their_protocol` and `Peer.splits` (task sync-divergence-check/
//! protocol-mismatch) encode and decode to themselves, and an older daemon's reply reads as "same
//! protocol" (0).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::SyncStatusResponse;
use txtodo_proto::v1::sync_status_response::Peer;

#[test]
fn a_peer_on_another_protocol_round_trips_beside_ours() {
    let status = SyncStatusResponse {
        peers: vec![Peer {
            device: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
            their_protocol: 3,
            ..Peer::default()
        }],
        pending_ops: 4,
        protocol: 2,
    };
    let back = SyncStatusResponse::decode(status.encode_to_vec().as_slice()).unwrap();
    assert_eq!(back, status);
}

#[test]
fn an_older_daemons_reply_reads_as_the_same_protocol() {
    // What an older daemon sends: every field it knows, and neither new field.
    let older = SyncStatusResponse {
        peers: vec![Peer {
            device: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
            lag_ms: 5,
            ..Peer::default()
        }],
        ..SyncStatusResponse::default()
    };
    let back = SyncStatusResponse::decode(older.encode_to_vec().as_slice()).unwrap();
    assert_eq!((back.protocol, back.peers[0].their_protocol), (0, 0));
}

#[test]
fn a_split_file_round_trips_on_its_peer() {
    use txtodo_proto::v1::sync_status_response::Split;
    let status = SyncStatusResponse {
        peers: vec![Peer {
            device: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
            splits: vec![Split {
                workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBW".into(),
                file: "tasks/a/todo.txt".into(),
                since_ms: 1_000,
            }],
            ..Peer::default()
        }],
        protocol: 3,
        ..SyncStatusResponse::default()
    };
    let back = SyncStatusResponse::decode(status.encode_to_vec().as_slice()).unwrap();
    assert_eq!(back, status);
}
