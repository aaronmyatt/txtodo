//! `UniversalTasks` over two real open workspaces (task `tui-revamp/universal-rpc`): a done line,
//! a due line and a `ref:` line with a sub-list and notes, through the real `GlobalService`.

use std::path::Path;
use std::sync::Arc;

use txtodo_proto::v1 as pb;
use txtodo_proto::v1::txtodo_server::Txtodo;

use crate::global_service::GlobalService;
use crate::workspace_catalog::WorkspaceCatalog;
use crate::workspace_catalog_load_tests::{catalog_with, select, workspace};

fn write(dir: &Path, path: &str, text: &str) {
    let path = dir.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("{e}"));
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{e}"));
}

async fn open(catalog: &Arc<WorkspaceCatalog>, dir: &Path) {
    let catalog = Arc::clone(catalog);
    let selector = select(dir);
    tokio::task::spawn_blocking(move || catalog.resolve(Some(&selector)).map(|_| ()))
        .await
        .unwrap_or_else(|e| panic!("open task: {e}"))
        .unwrap_or_else(|e| panic!("open: {e}"));
}

async fn rows(service: &GlobalService, include_done: bool) -> Vec<pb::UniversalTask> {
    service
        .universal_tasks(tonic::Request::new(pb::UniversalTasksRequest {
            include_done,
        }))
        .await
        .unwrap_or_else(|e| panic!("universal_tasks: {e}"))
        .into_inner()
        .tasks
}

fn find<'a>(rows: &'a [pb::UniversalTask], words: &str) -> &'a pb::UniversalTask {
    rows.iter()
        .find(|r| r.raw.contains(words))
        .unwrap_or_else(|| panic!("no row with {words:?}: {rows:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rows_from_every_open_workspace_with_done_due_and_ref_facts() {
    let (one, two) = (workspace("universal-one-"), workspace("universal-two-"));
    write(
        one.path(),
        "todo.txt",
        "(A) call mum @phone +family due:2026-09-26\n\
         x 2026-09-20 2026-09-01 file taxes pri:B\n\
         \n\
         plan trip ref:trip-plan\n",
    );
    write(
        one.path(),
        "tasks/trip-plan/todo.txt",
        "x 2026-09-24 book flights\nbook hotel\n",
    );
    write(one.path(), "tasks/trip-plan/notes.md", "ideas\n");
    write(two.path(), "todo.txt", "(C) water plants\n");
    let (_registry_dir, catalog) = catalog_with(&[], |_| {});
    for dir in [&one, &two] {
        open(&catalog, dir.path()).await;
    }
    let service = GlobalService::new(Arc::clone(&catalog));

    let open_rows = rows(&service, false).await;
    assert_eq!(open_rows.len(), 3, "{open_rows:?}");
    assert!(open_rows.iter().all(|r| !r.done));
    let mum = find(&open_rows, "call mum");
    assert_eq!(mum.line_number, 1);
    assert_eq!(mum.priority, "A");
    assert_eq!(mum.due, "2026-09-26", "the raw value; clients bucket it");
    assert_eq!(mum.contexts, ["phone"]);
    assert_eq!(mum.projects, ["family"]);
    assert!(mum.workspace_name.starts_with("universal-one-"));
    assert_eq!(mum.root_list, "todo.txt");
    assert!(!mum.task_id.is_empty(), "the daemon's id for the line");
    let trip = find(&open_rows, "plan trip");
    assert_eq!(trip.line_number, 4, "blank lines count, as in a TaskRef");
    assert!(trip.has_ref && trip.has_notes);
    assert_eq!(trip.ref_progress, Some(pb::Progress { done: 1, total: 2 }));
    let plants = find(&open_rows, "water plants");
    assert!(plants.workspace_name.starts_with("universal-two-"));
    assert!(!plants.has_ref && plants.ref_progress.is_none());

    let all_rows = rows(&service, true).await;
    assert_eq!(all_rows.len(), 4);
    let taxes = find(&all_rows, "file taxes");
    assert!(taxes.done);
    assert_eq!(taxes.completion_date, "2026-09-20");
    assert_eq!(taxes.priority, "B", "a done line's pri: tag counts");
}
