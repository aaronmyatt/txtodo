//! security-m8-review gap 1 (checklist item 1, re-checked at M8 scope): a tracing-capture test
//! over the three surfaces that did not exist at M4 -- relay, file carrier, bundle -- shaped
//! exactly like `lan_session_security_tests.rs`'s M4 test (same `LogSink`/`capturing_dispatch`/
//! `captured_text`/`hex` helpers, reused via their `pub(crate)` visibility rather than duplicated).
//! See `tasks/security-m8-review/notes.md`.
//!
//! Each cycle uses real secret material -- a real group key, a real device signing key (one fixed,
//! one freshly minted through the real keystore), a real bundle passphrase -- so a leak would show
//! up as exactly the bytes an attacker would want, not a stand-in. None of relay/file-carrier/
//! bundle's own code logs a payload today (`relay`'s `tracing::` call sites carry only routing
//! metadata/lengths; `carrier.rs` calls `tracing::` nowhere at all; `bundle_*.rs` calls it nowhere
//! either) -- this test is the regression guard for that staying true, not a report of a bug it
//! found.

use std::sync::Arc;

use tempfile::tempdir;
use txtodo_model::{DeviceId, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid};
use txtodo_store::WorkspaceId;
use txtodo_sync::{
    DeviceSigningKey, FileCarrier, Frame, GroupId, GroupKey, Link, OriginRange, SealContext,
    seal_ops,
};

use txtodo_telemetry::testing::{LogSink, capturing_dispatch, pin_global_trace_floor};

use crate::bundle_export::{ExportCtx, export_into};
use crate::bundle_import::{ImportCtx, import_from_chunks};
use crate::clock::FakeClock;
use crate::keystore_setup::load_or_mint_device_signing;
use crate::lan_session_security_tests::{SERVICE, hex};
use crate::mutation::Mutation;
use crate::workspace::Workspace;

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

/// A real signed+sealed `Ops` frame carrying a distinctive plaintext line -- exactly the shape
/// `lan_session.rs`/`bundle_export.rs` produce via the same `seal_ops`, just driven directly here
/// so this test controls every byte it later checks logs for. What "a relay put/get cycle" and "a
/// file-carrier send/recv cycle" both actually move.
fn sealed_frame(
    device: DeviceId,
    plaintext_line: &str,
    key: &GroupKey,
    signing: &DeviceSigningKey,
) -> Frame {
    let op = Op {
        id: OpId::new(Ulid::from_u128(0xAB)),
        hlc: Hlc {
            wall_ms: 1_700_000_000_000,
            counter: 0,
            device,
        },
        principal: Principal::User { device },
        file: FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")),
        kind: OpKind::Insert {
            task: TaskId::new(Ulid::from_u128(0xCD)),
            after: None,
            line: plaintext_line.to_owned(),
        },
    };
    let range = OriginRange {
        device,
        first: 1,
        last: 1,
    };
    let ctx = SealContext {
        group: GroupId(7),
        epoch: 0,
        workspace: WorkspaceId::new(Ulid::from_u128(0xAB)),
        key,
    };
    seal_ops(vec![op], vec![range], signing, &ctx).unwrap_or_else(|e| panic!("seal_ops: {e}"))
}

/// The relay put/get cycle (relay-reference, M8): a real sealed blob through the real
/// `relay::store::Store`, then its queued wake-up drained through the real `relay::push::NoopPush`
/// -- the same steps `http.rs`'s `put_blob` handler drives (already proven real end to end by
/// `relay/tests/http_smoke.rs`; this test's own job is the logging, not re-proving the HTTP
/// wiring).
fn relay_put_get_cycle(sealed: &[u8]) {
    use relay::push::Push as _;

    let dir = tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut store = relay::store::Store::open(&dir.path().join("relay.db"))
        .unwrap_or_else(|e| panic!("open relay store: {e}"));
    let limits = relay::store::Limits::default();
    store
        .put(
            relay::store::Write {
                group: "g1",
                device: "d1",
                blob: sealed,
                now_ms: 1_000,
            },
            limits,
        )
        .unwrap_or_else(|e| panic!("relay put: {e}"));
    let got = store
        .get("g1", "d1")
        .unwrap_or_else(|e| panic!("relay get: {e}"));
    assert_eq!(got.len(), 1, "the put blob must be gettable back");
    assert_eq!(
        got[0].blob, sealed,
        "relay stores/returns the blob byte-for-byte"
    );

    let pending = store
        .pending_wakeups("d1")
        .unwrap_or_else(|e| panic!("pending wakeups: {e}"));
    let mut push = relay::push::NoopPush::default();
    for wake in pending {
        push.wake(&"d1".to_owned(), &wake.payload)
            .unwrap_or_else(|e| panic!("push wake: {e}"));
        store
            .remove_wakeup(wake.id)
            .unwrap_or_else(|e| panic!("remove wakeup: {e}"));
    }
}

/// The file-carrier send/recv cycle (sync-file-carrier, M8): two real `FileCarrier`s over one
/// shared temp directory, one real sealed frame sent and received -- no network, design §4.5.
/// (Convergence itself, and the on-disk-ciphertext assertion, are `carrier_tests.rs`'s job; this
/// cycle exists only to give the logging capture something real to run over.)
fn file_carrier_send_recv_cycle(device_a: DeviceId, device_b: DeviceId, frame: Frame) {
    let dir = tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut carrier_a =
        FileCarrier::open(dir.path(), device_a).unwrap_or_else(|e| panic!("open a: {e}"));
    let mut carrier_b =
        FileCarrier::open(dir.path(), device_b).unwrap_or_else(|e| panic!("open b: {e}"));
    carrier_a
        .send(frame)
        .unwrap_or_else(|e| panic!("send: {e}"));
    let _received = carrier_b.recv().unwrap_or_else(|e| panic!("recv: {e}"));
}

/// The bundle export/import cycle (cli-bundle, M8): a real workspace, exported to a fresh one
/// under a real passphrase wrap and a real minted per-workspace device signing key -- the same
/// in-process core `bundle_tests.rs` exercises for its own `@test` items. Returns the raw bytes of
/// the signing key this cycle minted, read back from the real key store the same way
/// `lan_session_security_tests.rs`'s `secret_bytes_at` does, so the caller can check for exactly
/// the bytes a leak would contain.
async fn bundle_export_import_cycle(passphrase: &[u8], plaintext_line: &str) -> Vec<u8> {
    let dir_a = tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let dir_b = tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap_or_else(|e| panic!("touch: {e}"));

    let ws_a = Workspace::open(dir_a.path(), Arc::new(FakeClock::new(1_000)))
        .unwrap_or_else(|e| panic!("open a: {e}"));
    let handle = ws_a
        .actor(&FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}")))
        .unwrap_or_else(|| panic!("actor"));
    handle
        .apply(
            vec![Mutation::Add {
                line: plaintext_line.to_owned(),
            }],
            Principal::User {
                device: ws_a.device(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"));

    let bundle_signing = load_or_mint_device_signing(ws_a.key_store().as_ref())
        .unwrap_or_else(|e| panic!("mint signing key: {e}"));
    let bundle_signing_bytes = ws_a
        .key_store()
        .get(txtodo_sync::KeyId::DeviceSigning)
        .unwrap_or_else(|e| panic!("key store read: {e:?}"))
        .unwrap_or_else(|| panic!("signing key must have just been minted"))
        .expose()
        .to_vec();

    let export_ctx = ExportCtx {
        root: ws_a.root().to_path_buf(),
        store: ws_a.store().clone(),
        device: ws_a.device(),
        signing: bundle_signing,
        extra_document: None,
    };
    let mut frames = Vec::new();
    let mut emit = |data: Vec<u8>| {
        frames.push(data);
        Ok(())
    };
    export_into(&export_ctx, passphrase, &mut emit).unwrap_or_else(|e| panic!("export: {e}"));

    let ws_b = Workspace::open(dir_b.path(), Arc::new(FakeClock::new(1_000)))
        .unwrap_or_else(|e| panic!("open b: {e}"));
    let import_ctx = ImportCtx {
        root: ws_b.root().to_path_buf(),
        store: ws_b.store().clone(),
        now_ms: 5_000,
    };
    let mut it = frames.into_iter();
    let mut next = move || Ok(it.next());
    import_from_chunks(&import_ctx, passphrase, &mut next)
        .unwrap_or_else(|e| panic!("import: {e}"));

    bundle_signing_bytes
}

fn assert_no_secret_leaked(sink: &LogSink, secrets: &[(&str, &[u8])]) {
    let logs = sink.captured_text();
    // Without this, a run that captured nothing passes every check below and proves nothing.
    assert!(
        !logs.is_empty(),
        "sanity: the relay, file-carrier and bundle round actually logged something"
    );
    for (name, bytes) in secrets {
        assert!(
            !bytes.is_empty(),
            "{name}: fixture bug, an empty secret proves nothing"
        );
        assert!(!logs.contains(&hex(bytes)), "{name} hex leaked into logs");
        let lossy = String::from_utf8_lossy(bytes).into_owned();
        assert!(
            lossy.is_empty() || !logs.contains(&lossy),
            "{name} raw bytes leaked into logs"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn no_secrets_appear_in_logs_across_relay_file_carrier_and_bundle() {
    // Same reason as `lan_session_security_tests.rs`: sibling tests in this binary log with no
    // subscriber of their own (task tracing-set-default-audit).
    pin_global_trace_floor();
    let sink = LogSink::new();
    let dispatch = capturing_dispatch(sink.clone(), SERVICE);
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let device_a = dev(1);
    let device_b = dev(2);
    let group_key_bytes = [0x7Au8; 32];
    let group_key = GroupKey::from_bytes(group_key_bytes);
    let signing_key_bytes = [0x33u8; 32];
    let signing_key = DeviceSigningKey::from_bytes(signing_key_bytes);
    let plaintext_line = "DEFINITELY-PLAINTEXT-buy-plutonium-for-the-reactor";
    let passphrase: &[u8] = b"correct horse battery staple (m8 security review)";

    // Relay + file carrier: both move the identical real sealed frame, the same way the real
    // wire/disk path does.
    let sealed = sealed_frame(device_a, plaintext_line, &group_key, &signing_key);
    let sealed_bytes = sealed.body.clone();
    relay_put_get_cycle(&sealed_bytes);
    file_carrier_send_recv_cycle(device_a, device_b, sealed);

    // Bundle: export on a real workspace, import on a fresh one.
    let bundle_signing_bytes = bundle_export_import_cycle(passphrase, plaintext_line).await;

    drop(_guard);
    assert_no_secret_leaked(
        &sink,
        &[
            ("group key", &group_key_bytes),
            ("relay/file-carrier device signing key", &signing_key_bytes),
            ("bundle device signing key", &bundle_signing_bytes),
            ("bundle passphrase", passphrase),
            ("plaintext task line", plaintext_line.as_bytes()),
        ],
    );
}
