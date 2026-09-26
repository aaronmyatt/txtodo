//! `WorkspaceRename` over `GlobalService` (task workspace-vanity-name): whitebox, same-process, a
//! real catalog over a real temp workspace. The name lands in `txtodo.toml` as ops, the answer and
//! `WorkspaceList` show it, and a rename is the old line out, then the new one in, so two renames
//! made at once merge into two whole lines.

use std::path::Path;
use std::sync::PoisonError;

use tonic::{Code, Request};
use txtodo_crdt::NotesDoc;
use txtodo_model::{FilePath, OpKind, TextEdit};
use txtodo_proto::v1::{self as pb, txtodo_server::Txtodo};
use txtodo_store::Seq;

use crate::global_service::GlobalService;
use crate::workspace_catalog_load_tests::{catalog_with, select, workspace};
use crate::workspace_name::{parse, rename_steps};

async fn service(root: &Path) -> (tempfile::TempDir, GlobalService, String) {
    let (registry_dir, catalog) = catalog_with(&[root], |_| {});
    let svc = GlobalService::new(catalog);
    let ws = svc.resolve(Some(&select(root))).await.unwrap();
    let id = ws
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .workspace_id();
    (registry_dir, svc, id.to_string())
}

async fn rename(svc: &GlobalService, id: &str, name: &str) -> Result<pb::WorkspaceInfo, Code> {
    let req = pb::WorkspaceRenameRequest {
        workspace_id: id.to_owned(),
        name: name.to_owned(),
    };
    svc.workspace_rename(Request::new(req))
        .await
        .map(tonic::Response::into_inner)
        .map_err(|s| s.code())
}

/// Each `NotesEdit` op on `txtodo.toml`, as its edits.
async fn layout_ops(svc: &GlobalService, root: &Path) -> Vec<Vec<TextEdit>> {
    let ws = svc.resolve(Some(&select(root))).await.unwrap();
    let ws = ws.read().unwrap_or_else(PoisonError::into_inner);
    let path = FilePath::new("txtodo.toml").unwrap();
    let store = ws.store().lock().unwrap_or_else(PoisonError::into_inner);
    let ops = store.for_file(&path, Seq(0)).unwrap();
    ops.into_iter()
        .filter_map(|s| match s.op.kind {
            OpKind::NotesEdit { edits, .. } => Some(edits),
            _ => None,
        })
        .collect()
}

fn layout_file(root: &Path) -> String {
    std::fs::read_to_string(root.join("txtodo.toml")).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_writes_the_name_and_every_listing_shows_it() {
    let ws = workspace("plants-");
    let (_registry, svc, id) = service(ws.path()).await;

    let info = rename(&svc, &id, "  House plants ").await.unwrap();
    assert_eq!(info.name, "House plants");
    let text = layout_file(ws.path());
    assert!(text.contains("refs_dir = "), "layout too:\n{text}");
    assert!(text.ends_with("name = \"House plants\"\n"), "{text}");
    let req = Request::new(pb::WorkspaceListRequest {});
    let listed = svc.workspace_list(req).await.unwrap().into_inner();
    let row = listed.workspaces.iter().find(|w| w.workspace_id == id);
    assert_eq!(row.map(|w| w.name.as_str()), Some("House plants"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_is_the_old_line_out_then_the_new_one_in() {
    let ws = workspace("plants-");
    std::fs::write(ws.path().join("txtodo.toml"), "name = \"Old\"\n").unwrap();
    let (_registry, svc, id) = service(ws.path()).await;
    let before = layout_ops(&svc, ws.path()).await.len();

    rename(&svc, &id, "New").await.unwrap();
    let ops = layout_ops(&svc, ws.path()).await;
    let out = TextEdit::Delete { at: 0, len: 13 };
    let into = TextEdit::Insert {
        at: 0,
        text: "name = \"New\"\n".to_owned(),
    };
    assert_eq!(ops[before..], [vec![out], vec![into]], "never one diff");
    // The same name again changes nothing; an empty one clears it, back to the folder's name.
    rename(&svc, &id, "New").await.unwrap();
    assert_eq!(layout_ops(&svc, ws.path()).await.len(), before + 2);
    let cleared = rename(&svc, &id, "").await.unwrap();
    assert_eq!(layout_file(ws.path()), "");
    let folder = ws.path().file_name().unwrap().to_string_lossy();
    assert_eq!(cleared.name, folder);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bad_name_or_an_unknown_workspace_is_refused() {
    let ws = workspace("plants-");
    let (_registry, svc, id) = service(ws.path()).await;
    let refused = |r: Result<pb::WorkspaceInfo, Code>| r.err();
    let bad = rename(&svc, &id, "two\nlines").await;
    assert_eq!(refused(bad), Some(Code::InvalidArgument));
    let not_an_id = rename(&svc, "not-a-ulid", "x").await;
    assert_eq!(refused(not_an_id), Some(Code::InvalidArgument));
    let unknown = rename(&svc, "01M2RZ8EX1CQAS21TNZ5YY6PBT", "x").await;
    assert_eq!(refused(unknown), Some(Code::NotFound));
    std::fs::write(ws.path().join("txtodo.toml"), "\"name\" = \"hand\"\n").unwrap();
    let hand = rename(&svc, &id, "x").await;
    assert_eq!(refused(hand), Some(Code::FailedPrecondition));
    assert_eq!(layout_file(ws.path()), "\"name\" = \"hand\"\n", "untouched");
}

/// `doc` taken through `rename_steps`, one Loro commit per step, as the notes actor does.
fn rename_doc(doc: &mut NotesDoc, name: &str) {
    for step in rename_steps(&doc.content(), Some(name)).unwrap() {
        let edits: Vec<TextEdit> = txtodo_core::diff_text(&doc.content(), &step)
            .into_iter()
            .map(TextEdit::from)
            .collect();
        doc.apply_edits(&edits).unwrap();
    }
}

#[test]
fn two_renames_at_once_merge_into_two_whole_lines_and_one_name() {
    let base = NotesDoc::hydrate("refs_dir = \"tasks\"\nname = \"Old\"\n").unwrap();
    let snapshot = base.snapshot().unwrap();
    let fork = |peer: u64| {
        let doc = NotesDoc::from_snapshot(&snapshot).unwrap();
        doc.set_peer(peer).unwrap();
        doc
    };
    let (mut a, mut b) = (fork(1), fork(2));
    rename_doc(&mut a, "Groceries");
    rename_doc(&mut b, "Shopping");
    let (from_a, from_b) = (a.snapshot().unwrap(), b.snapshot().unwrap());
    a.import(&from_b).unwrap();
    b.import(&from_a).unwrap();

    let text = a.content();
    assert_eq!(text, b.content(), "both devices hold one text");
    assert!(text.contains("name = \"Groceries\"\n"), "{text}");
    assert!(text.contains("name = \"Shopping\"\n"), "{text}");
    assert!(!text.contains("Old"), "{text}");
    let name = parse(&text).unwrap();
    assert!(["Groceries", "Shopping"].contains(&name.as_str()), "{name}");
    assert!(crate::layout_file::parse(&text).is_ok(), "the layout loads");
}
