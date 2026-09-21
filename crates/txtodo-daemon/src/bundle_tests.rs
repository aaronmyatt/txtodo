//! End-to-end coverage for `txtodo bundle export|import`'s daemon-side core (plan M8
//! `cli-bundle`, design §4.5): the `@test` items from `tasks/cli-bundle/todo.txt`, driven
//! directly against `export_into`/`import_from_chunks` — in-process, no socket, no CLI binary,
//! the same "whitebox is the right tool here" call this crate's other `*_tests.rs` files make
//! (`pairing_grpc_tests.rs`'s own module doc has the same reasoning for the same reason: this is
//! about the crypto/data-shape guarantees, not the transport).

use std::path::Path;
use std::sync::Arc;

use prost::Message;
use txtodo_model::{FilePath, Principal};
use txtodo_proto::v1 as pb;
use txtodo_store::Seq;

use crate::actor::SharedStore;
use crate::bundle_crypto::CHUNK_PLAINTEXT_MAX;
use crate::bundle_export::{ExportCtx, export_into};
use crate::bundle_import::{ImportCtx, ImportOutcome, import_from_chunks};
use crate::bundle_import_error::BundleImportError;
use crate::clock::FakeClock;
use crate::keystore_setup::load_or_mint_device_signing;
use crate::mutation::Mutation;
use crate::workspace::Workspace;

const PASSPHRASE: &[u8] = b"correct horse battery staple";

fn touch(p: &Path, bytes: &str) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{e}"));
    }
    std::fs::write(p, bytes).unwrap_or_else(|e| panic!("{e}"));
}

fn open(root: &Path) -> Workspace {
    Workspace::open(root, Arc::new(FakeClock::new(1_000))).unwrap_or_else(|e| panic!("open: {e}"))
}

/// Appends every `line` to `path` in one `Apply` batch (one HLC tick, one store transaction).
async fn seed(ws: &Workspace, path: &str, lines: &[&str]) {
    let handle = ws.actor(&FilePath::new(path).unwrap()).unwrap();
    let principal = Principal::User {
        device: ws.device(),
    };
    let mutations = lines
        .iter()
        .map(|l| Mutation::Add {
            line: (*l).to_owned(),
        })
        .collect();
    handle
        .apply(mutations, principal)
        .await
        .unwrap_or_else(|e| panic!("apply: {e}"));
}

fn export_ctx(ws: &Workspace) -> ExportCtx {
    let signing = load_or_mint_device_signing(ws.key_store().as_ref())
        .unwrap_or_else(|e| panic!("mint signing key: {e}"));
    ExportCtx {
        root: ws.root().to_path_buf(),
        store: ws.store().clone(),
        device: ws.device(),
        signing,
        extra_document: ws.extra_document(),
    }
}

/// A full export into an in-memory list of raw frames, exactly as they would ride one `BundleChunk`
/// wire message each.
fn export_to_frames(ws: &Workspace, passphrase: &[u8]) -> Vec<Vec<u8>> {
    let ctx = export_ctx(ws);
    let mut frames = Vec::new();
    let mut emit = |data: Vec<u8>| {
        frames.push(data);
        Ok(())
    };
    export_into(&ctx, passphrase, &mut emit).unwrap_or_else(|e| panic!("export: {e}"));
    frames
}

/// Feeds `frames` into `import_from_chunks` against the workspace at `root`/`store`.
fn import_frames(
    root: &Path,
    store: SharedStore,
    passphrase: &[u8],
    frames: Vec<Vec<u8>>,
) -> Result<ImportOutcome, BundleImportError> {
    let ctx = ImportCtx {
        root: root.to_path_buf(),
        store,
        now_ms: 5_000,
    };
    let mut it = frames.into_iter();
    let mut next = move || Ok(it.next());
    import_from_chunks(&ctx, passphrase, &mut next)
}

fn ops_of(ws: &Workspace, file: &str) -> Vec<txtodo_store::Stored> {
    let file = FilePath::new(file).unwrap();
    ws.store()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .for_file(&file, Seq(0))
        .unwrap_or_else(|e| panic!("read ops: {e}"))
}

/// @test: export on A, import on a fresh B gives identical file bytes and identical op-log state
/// (seq, op_id, HLC, principal).
#[tokio::test]
async fn export_a_import_fresh_b_are_byte_identical_with_the_same_op_log() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    touch(&dir_a.path().join("todo.txt"), "");
    let ws_a = open(dir_a.path());
    seed(
        &ws_a,
        "todo.txt",
        &["(A) buy ducks +farm", "water the plants"],
    )
    .await;

    let frames = export_to_frames(&ws_a, PASSPHRASE);
    let mut ws_b = open(dir_b.path());
    let outcome = import_frames(dir_b.path(), ws_b.store().clone(), PASSPHRASE, frames)
        .unwrap_or_else(|e| panic!("import: {e}"));
    let ops_a = ops_of(&ws_a, "todo.txt");
    assert_eq!(outcome.ops_imported, ops_a.len() as u64);
    ws_b.discover(dir_b.path())
        .unwrap_or_else(|e| panic!("discover: {e}"));

    let bytes_a = std::fs::read(dir_a.path().join("todo.txt")).unwrap();
    let bytes_b = std::fs::read(dir_b.path().join("todo.txt")).unwrap();
    assert_eq!(bytes_a, bytes_b, "byte-identical file");

    let ops_b = ops_of(&ws_b, "todo.txt");
    assert_eq!(ops_a, ops_b, "identical seq, op_id, HLC and principal");
}

/// @test: one flipped byte anywhere fails import with a distinct stage error and leaves no
/// partial state. Flips one byte of a `FileHash.blake3` inside the *clear* manifest — a
/// tamper the AEAD layer never sees, so it must be caught by the hash re-check instead.
#[tokio::test]
async fn one_flipped_manifest_hash_byte_fails_distinctly_with_no_partial_state() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    touch(&dir_a.path().join("todo.txt"), "");
    let ws_a = open(dir_a.path());
    seed(&ws_a, "todo.txt", &["buy ducks"]).await;

    let mut frames = export_to_frames(&ws_a, PASSPHRASE);
    let mut manifest = pb::BundleManifest::decode(frames[1].as_slice()).unwrap();
    manifest.files[0].blake3[0] ^= 0x01;
    frames[1] = manifest.encode_to_vec();

    let ws_b = open(dir_b.path());
    let err = import_frames(dir_b.path(), ws_b.store().clone(), PASSPHRASE, frames).unwrap_err();
    assert!(
        matches!(err, BundleImportError::FileHashMismatch { .. }),
        "{err:?}"
    );
    assert!(
        !dir_b.path().join("todo.txt").exists(),
        "nothing written on failure"
    );
    assert_eq!(
        ws_b.store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .total_ops()
            .unwrap(),
        0
    );
}

/// @test: wrong-passphrase import fails without writing any state; the clear manifest carries no
/// key material (also checks the key-free decision is enforced in the wire format itself).
#[tokio::test]
async fn wrong_passphrase_writes_nothing_and_the_manifest_carries_no_key_material() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    touch(&dir_a.path().join("todo.txt"), "");
    let ws_a = open(dir_a.path());
    seed(&ws_a, "todo.txt", &["buy ducks"]).await;

    let frames = export_to_frames(&ws_a, PASSPHRASE);
    // The clear manifest decodes without ever deriving a key: `BundleManifest` has no key field
    // at all (plan M8's 2026-09-13 key-free decision) — only a *public* signing key, which is
    // exactly as public as `PairOfferResponse.x25519_pub` and never the group key.
    let manifest = pb::BundleManifest::decode(frames[1].as_slice()).unwrap();
    assert_eq!(manifest.device_signing_public_key.len(), 32);

    let ws_b = open(dir_b.path());
    let err = import_frames(
        dir_b.path(),
        ws_b.store().clone(),
        b"definitely the wrong passphrase",
        frames,
    )
    .unwrap_err();
    assert!(matches!(err, BundleImportError::Decrypt(_)), "{err:?}");
    assert!(!dir_b.path().join("todo.txt").exists());
    assert_eq!(
        ws_b.store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .total_ops()
            .unwrap(),
        0
    );
}

/// @test: a nested-`ref:` workspace round-trips through export/import, reproducing the whole tree.
#[tokio::test]
async fn a_nested_ref_workspace_round_trips_the_whole_tree() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    touch(&dir_a.path().join("todo.txt"), "");
    touch(&dir_a.path().join("q4-roadmap/todo.txt"), "");
    touch(&dir_a.path().join("q4-roadmap/deep/todo.txt"), "");
    touch(&dir_a.path().join("q4-roadmap/notes.md"), "# child notes\n");
    let ws_a = open(dir_a.path());
    seed(&ws_a, "todo.txt", &["parent task"]).await;
    seed(&ws_a, "q4-roadmap/todo.txt", &["child task"]).await;
    seed(&ws_a, "q4-roadmap/deep/todo.txt", &["grandchild task"]).await;

    let frames = export_to_frames(&ws_a, PASSPHRASE);
    let mut ws_b = open(dir_b.path());
    let outcome = import_frames(dir_b.path(), ws_b.store().clone(), PASSPHRASE, frames)
        .unwrap_or_else(|e| panic!("import: {e}"));
    assert_eq!(outcome.files.len(), 4, "3 todo.txt + 1 notes.md");
    ws_b.discover(dir_b.path())
        .unwrap_or_else(|e| panic!("discover: {e}"));

    for rel in [
        "todo.txt",
        "q4-roadmap/todo.txt",
        "q4-roadmap/deep/todo.txt",
        "q4-roadmap/notes.md",
    ] {
        let a = std::fs::read(dir_a.path().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let b = std::fs::read(dir_b.path().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(a, b, "{rel}");
    }
}

/// @test: export of a large op tail streams in bounded chunks and stays within a fixed memory
/// budget — checked structurally: many chunks, each at most `CHUNK_PLAINTEXT_MAX` (+ the AEAD
/// tag) of ciphertext, never one giant blob.
#[tokio::test]
async fn a_large_op_tail_streams_in_multiple_bounded_chunks() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    touch(&dir_a.path().join("todo.txt"), "");
    let ws_a = open(dir_a.path());
    let lines: Vec<String> = (0..2_000)
        .map(|i| format!("task number {i} +bulk"))
        .collect();
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    seed(&ws_a, "todo.txt", &line_refs).await;

    let frames = export_to_frames(&ws_a, PASSPHRASE);
    assert!(
        frames.len() > 5,
        "a 2000-op tail must split across several chunks, got {}",
        frames.len()
    );
    for (i, frame) in frames.iter().enumerate().skip(2) {
        assert!(
            frame.len() <= CHUNK_PLAINTEXT_MAX + 32,
            "chunk {i} is {} bytes, over the bound",
            frame.len()
        );
    }

    let mut ws_b = open(dir_b.path());
    let outcome = import_frames(dir_b.path(), ws_b.store().clone(), PASSPHRASE, frames)
        .unwrap_or_else(|e| panic!("import: {e}"));
    assert_eq!(outcome.ops_imported, 2_000);
    ws_b.discover(dir_b.path())
        .unwrap_or_else(|e| panic!("discover: {e}"));
    let bytes_a = std::fs::read(dir_a.path().join("todo.txt")).unwrap();
    let bytes_b = std::fs::read(dir_b.path().join("todo.txt")).unwrap();
    assert_eq!(bytes_a, bytes_b);
}

/// Re-importing an already-landed bundle is idempotent: duplicate `op_id`s are skipped, not
/// re-applied (design §4.5's own invariant, backed by the `ops.op_id` UNIQUE index).
#[tokio::test]
async fn reimporting_the_same_bundle_is_idempotent() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    touch(&dir_a.path().join("todo.txt"), "");
    let ws_a = open(dir_a.path());
    seed(&ws_a, "todo.txt", &["buy ducks", "water the plants"]).await;

    let frames = export_to_frames(&ws_a, PASSPHRASE);
    let ws_b = open(dir_b.path());
    let first = import_frames(
        dir_b.path(),
        ws_b.store().clone(),
        PASSPHRASE,
        frames.clone(),
    )
    .unwrap_or_else(|e| panic!("first import: {e}"));
    assert_eq!(first.ops_imported, 2);
    let second = import_frames(dir_b.path(), ws_b.store().clone(), PASSPHRASE, frames)
        .unwrap_or_else(|e| panic!("second import: {e}"));
    assert_eq!(second.ops_imported, 0, "every op_id already present");
    assert_eq!(
        ws_b.store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .total_ops()
            .unwrap(),
        2,
        "no duplicate rows landed"
    );
}
