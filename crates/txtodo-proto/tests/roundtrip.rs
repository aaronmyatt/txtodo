//! Every message encodes and decodes to itself; the oneof keeps its variant. Guards the committed
//! generated code against a stale regeneration.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{
    Add, AgentPrincipal, ApplyRequest, ApplyResponse, Change, CheckoutRequest, Complete, Delete,
    Device, DeviceListRequest, DeviceListResponse, DeviceRemoveRequest, DeviceRemoveResponse, Edit,
    FileContents, FileInfo, FileKind, GetFileRequest, HealthResponse, HistoryRequest,
    HistoryResponse, ListFilesResponse, Move, MoveToEnd, Mutation, OpSummary, Progress, SkewStatus,
    TaskRef, TreeNode, UndoRequest, WatchRequest, mutation,
};

fn round_trip<M: Message + Default + PartialEq + std::fmt::Debug>(m: &M) {
    let bytes = m.encode_to_vec();
    let back = M::decode(bytes.as_slice()).unwrap();
    assert_eq!(&back, m);
}

fn task() -> Option<TaskRef> {
    Some(TaskRef {
        line_number: 3,
        task_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
    })
}

fn sample_op() -> OpSummary {
    OpSummary {
        seq: 42,
        op_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        hlc_wall_ms: 1_700_000_000_000,
        hlc_counter: 7,
        device: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        principal: "you@01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        kind: "edit_text".into(),
        task_id: String::new(),
        summary: "buy ducks".into(),
    }
}

fn sample_file() -> FileInfo {
    FileInfo {
        path: "todo.txt".into(),
        hash: vec![1; 32],
        kind: FileKind::Todo as i32,
        progress: Some(Progress { done: 1, total: 3 }),
    }
}

#[test]
fn every_mutation_variant_survives_encode_decode() {
    let kinds = [
        mutation::Kind::Add(Add {
            line: "(A) 2026-09-11 buy ducks +farm".into(),
        }),
        mutation::Kind::Complete(Complete {
            task: task(),
            today: "2026-09-11".into(),
        }),
        mutation::Kind::Edit(Edit {
            task: task(),
            new_line: "(B) 2026-09-11 buy ducks +farm".into(),
        }),
        mutation::Kind::Move(Move {
            task: task(),
            to_path: "q4/todo.txt".into(),
        }),
        mutation::Kind::Delete(Delete {
            task: task(),
            leave_blank: true,
        }),
        mutation::Kind::MoveToEnd(MoveToEnd { task: task() }),
    ];
    for kind in kinds {
        let req = ApplyRequest {
            path: "todo.txt".into(),
            mutations: vec![Mutation {
                kind: Some(kind.clone()),
            }],
            agent: Some(AgentPrincipal {
                token_id: "t".into(),
                name: "claude".into(),
            }),
        };
        round_trip(&req);
        let decoded = ApplyRequest::decode(req.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.mutations[0].kind, Some(kind));
    }
}

#[test]
fn responses_and_streams_round_trip() {
    let op = sample_op();
    let file = sample_file();
    round_trip(&ListFilesResponse {
        files: vec![file.clone()],
        tree: Some(TreeNode {
            dir: String::new(),
            progress: Some(Progress { done: 1, total: 3 }),
            owner_task_id: String::new(),
            files: vec![file],
            children: Vec::new(),
        }),
    });
    round_trip(&FileContents {
        path: "todo.txt".into(),
        bytes: b"(A) x\r\n".to_vec(),
        hash: vec![2; 32],
    });
    round_trip(&Change {
        path: "todo.txt".into(),
        hash: vec![3; 32],
        ops: vec![op.clone()],
        review: Vec::new(),
        progress: Some(Progress { done: 1, total: 3 }),
    });
    round_trip(&HistoryResponse { ops: vec![op] });
    round_trip(&ApplyResponse {
        applied: 2,
        hash: vec![4; 32],
        hlc_wall_ms: 5,
        hlc_counter: 6,
    });
    round_trip(&HealthResponse {
        watcher_alive: true,
        documents: 3,
        last_event_age_ms: 10,
        started_at_ms: 20,
        writes_total: 30,
        version: "0.0.0".into(),
        key_store_backend: "os".into(),
        lan_relay_disabled: true,
        lan_endpoint_bound: true,
        lan_discovery_active: true,
        lan_group_key_present: false,
        relay_url: String::new(),
        relay_last_outcome: String::new(),
        pairing_last_carrier: String::new(),
    });
}

#[test]
fn requests_round_trip() {
    round_trip(&GetFileRequest {
        path: "todo.txt".into(),
    });
    round_trip(&WatchRequest {
        paths: vec!["todo.txt".into(), "q4/todo.txt".into()],
    });
    round_trip(&HistoryRequest {
        path: String::new(),
        task_id: "x".into(),
        limit: 50,
        before_seq: 0,
    });
    round_trip(&UndoRequest {
        path: "todo.txt".into(),
        steps: 2,
    });
    round_trip(&CheckoutRequest {
        path: "todo.txt".into(),
        at_wall_ms: 99,
    });
    round_trip(&DeviceListRequest {});
    round_trip(&DeviceRemoveRequest { id: "x".into() });
}

#[test]
fn device_messages_round_trip() {
    round_trip(&Device {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        name: "laptop".into(),
        is_self: false,
        removed: false,
        key_epoch: 2,
        paired_at_ms: 1_000,
        last_seen_ms: 2_000,
        skew_status: SkewStatus::Behind as i32,
        skew_ms: 400_000,
    });
    round_trip(&DeviceListResponse {
        devices: vec![Device::default()],
    });
    round_trip(&DeviceRemoveResponse {
        removed: true,
        already_removed: false,
        rotated_to_epoch: 3,
        message: "Rotated to key epoch 3.".into(),
    });
}
