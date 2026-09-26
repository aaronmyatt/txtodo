//! Every message encodes and decodes to itself; the oneof keeps its variant. Guards the committed
//! generated code against a stale regeneration.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{
    Add, AgentPrincipal, ApplyRequest, ApplyResponse, Change, CheckoutRequest, Complete, Delete,
    Device, DeviceListRequest, DeviceListResponse, DeviceRemoveRequest, DeviceRemoveResponse, Edit,
    FileContents, FileInfo, FileKind, GetFileRequest, HealthResponse, HistoryRequest,
    HistoryResponse, LintFinding, LintRequest, LintResponse, ListFilesResponse,
    MigrateIdentityResponse, Move, MoveBefore, MoveToEnd, Mutation, OpSummary, Progress, Reopen,
    Replace, RequireBase, SkewStatus, SyncStatusRequest, SyncStatusResponse, TaskRef, TreeNode,
    UndoRequest, WatchRequest, WorkspaceInfo, WorkspaceLoadState, mutation, sync_status_response,
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
        source: "cli".into(),
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
        mutation::Kind::Replace(Replace {
            base_hash: vec![7; 32],
            contents: b"(A) buy ducks\r\nwalk the dog".to_vec(),
        }),
        mutation::Kind::RequireBase(RequireBase {
            base_hash: vec![9; 32],
        }),
        mutation::Kind::Reopen(Reopen { task: task() }),
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
            workspace: None,
            source: "cli".into(),
            dry_run: true,
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
        bytes: b"(A) x\r\n\r\n".to_vec(),
        hash: vec![2; 32],
        // One entry per line: a task line's ULID text, "" for the blank line after it.
        task_ids: vec!["01M2T868JD32M84JQQ2ABASXW4".into(), String::new()],
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
        diff: "--- a/todo.txt\n+++ b/todo.txt\n".into(),
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
        ..HealthResponse::default()
    });
}

#[test]
fn requests_round_trip() {
    round_trip(&GetFileRequest {
        path: "todo.txt".into(),
        workspace: None,
    });
    round_trip(&WatchRequest {
        paths: vec!["todo.txt".into(), "q4/todo.txt".into()],
        workspace: None,
    });
    round_trip(&HistoryRequest {
        path: String::new(),
        task_id: "x".into(),
        limit: 50,
        before_seq: 0,
        workspace: None,
    });
    round_trip(&UndoRequest {
        path: "todo.txt".into(),
        steps: 2,
        workspace: None,
    });
    round_trip(&CheckoutRequest {
        path: "todo.txt".into(),
        at_wall_ms: 99,
        workspace: None,
    });
    round_trip(&DeviceListRequest { workspace: None });
    round_trip(&DeviceRemoveRequest {
        id: "x".into(),
        workspace: None,
    });
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
        own_device: true,
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

#[test]
fn sync_status_messages_round_trip() {
    round_trip(&SyncStatusRequest { workspace: None });
    round_trip(&SyncStatusResponse {
        peers: vec![sync_status_response::Peer {
            device: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            lag_ms: 400,
            stuck: vec![sync_status_response::Stuck {
                workspace_id: "01ARZ3NDEKTSV4RRFFQ69G5FAW".into(),
                file: "tasks/a/todo.txt".into(),
                reason: "store: UNIQUE constraint failed: ops.op_id".into(),
                since_ms: 1_000,
                last_ms: 21_000,
                refusals: 3,
            }],
            parked: true,
        }],
        pending_ops: 3,
    });
    // No peers, nothing pending — the empty case this RPC's own doc calls out.
    round_trip(&SyncStatusResponse::default());
}

/// Task sync-drift line 7: an older daemon's peer (fields 1-2 only) reads as not stuck and not
/// parked.
#[test]
fn an_older_daemons_peer_reads_as_not_stuck_and_not_parked() {
    let old = sync_status_response::Peer {
        device: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        lag_ms: 7,
        ..sync_status_response::Peer::default()
    };
    round_trip(&old);
    assert!(old.stuck.is_empty(), "nothing stuck");
    assert!(!old.parked, "not parked");
}

#[test]
fn the_early_bind_and_relay_fields_round_trip_and_default_to_absent() {
    let info = WorkspaceInfo {
        workspace_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        root: "/home/a/project".into(),
        load_state: WorkspaceLoadState::Failed as i32,
        load_error: "disk".into(),
        is_remote: true,
        ..WorkspaceInfo::default()
    };
    round_trip(&info);
    assert!(
        !WorkspaceInfo::default().is_remote,
        "an older daemon never claims a mirror"
    );
}

/// Task `default-workspace-pairing-consent`: the own-device answer and flag round-trip, and an
/// older peer's message reads as "not own" / "adopted".
#[test]
fn the_own_device_fields_round_trip_and_default_to_false() {
    use txtodo_proto::v1::{Device, PairConfirmRequest, PairResult};
    round_trip(&PairConfirmRequest {
        workspace: None,
        own_device: true,
    });
    round_trip(&Device {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        own_device: true,
        ..Device::default()
    });
    round_trip(&PairResult {
        sas: "one two three four five six".into(),
        kept_own_workspace: true,
    });
    assert!(!PairConfirmRequest::default().own_device);
    assert!(!Device::default().own_device);
    assert!(!PairResult::default().kept_own_workspace);
}

/// Task `control-channel-keystore-visibility`: the offers problem round-trips on both messages and
/// reads as "no problem" from an older daemon.
#[test]
fn the_offers_problem_round_trips_and_defaults_to_empty() {
    use txtodo_proto::v1::WorkspacePendingOffersResponse;
    round_trip(&HealthResponse {
        offers_problem: "keystore: did not answer".into(),
        offers_problem_age_ms: 1_500,
        ..HealthResponse::default()
    });
    round_trip(&WorkspacePendingOffersResponse {
        offers: Vec::new(),
        offers_problem: "keystore: did not answer".into(),
        offers_problem_age_ms: 1_500,
    });
    assert!(HealthResponse::default().offers_problem.is_empty());
    assert_eq!(
        WorkspaceInfo::default().load_state,
        WorkspaceLoadState::Unspecified as i32,
        "an older daemon says nothing"
    );

    let health = HealthResponse {
        workspaces_registered: 12,
        workspaces_ready: 3,
        workspaces_loading: 8,
        workspaces_failed: 1,
        relay_node_id: "ab".repeat(32),
        relay_bound: true,
        release_date: "2026-09-20".into(),
        ..HealthResponse::default()
    };
    round_trip(&health);
    let none = HealthResponse::default();
    assert!(none.relay_node_id.is_empty() && !none.relay_bound);
    assert!(
        none.release_date.is_empty(),
        "an older daemon sends no date"
    );

    round_trip(&MigrateIdentityResponse {
        paired_peers: 2,
        ..MigrateIdentityResponse::default()
    });
}

#[test]
fn move_before_and_lint_round_trip() {
    round_trip(&Mutation {
        kind: Some(mutation::Kind::MoveBefore(MoveBefore {
            task: task(),
            before: Some(TaskRef {
                line_number: 1,
                task_id: String::new(),
            }),
        })),
    });
    round_trip(&LintRequest {
        path: "todo.txt".into(),
        workspace: None,
    });
    round_trip(&LintResponse {
        findings: vec![
            LintFinding {
                line: 2,
                message: "101 chars, over the 100-char hint".into(),
            },
            LintFinding {
                line: 0,
                message: "byte order mark".into(),
            },
        ],
    });
}

#[test]
fn the_layout_messages_round_trip() {
    use txtodo_proto::v1::{WorkspaceLayoutInfo, WorkspaceLayoutRequest};
    round_trip(&WorkspaceLayoutRequest {
        workspace: None,
        set: true,
        refs_dir: "tasks".into(),
        todo_file: "todo.txt".into(),
        move_dirs: true,
    });
    round_trip(&WorkspaceLayoutInfo {
        refs_dir: "tasks".into(),
        todo_file: "todo.txt".into(),
        note: "refused".into(),
        moved: 2,
        outside_refs_dir: vec!["old/plan".into()],
    });
}
