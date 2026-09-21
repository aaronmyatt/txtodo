//! The in-memory `McpBackend` `tests/smoke.rs` runs the server against (split out for that file's
//! line budget): records every call and returns canned data — no daemon, no filesystem.

use std::sync::Mutex;

use txtodo_mcp::backend::{
    ApplyOutcome, ConflictFlag, ConflictSide, FieldPatch, FileMeta, GetTarget, Hlc, LintFinding,
    ListArgs, McpBackend, MoveAnchor, OpSummary, RefPath, TaskId, TaskRow, TodoOp, WorkspaceArg,
    WorkspaceInfo,
};
use txtodo_mcp::error::McpError;

/// Records every call it receives and returns canned, deterministic data — no real daemon, no
/// filesystem. Exercises the wiring (schema → tools → backend → back out as JSON), not the gRPC
/// client (that's `grpc_backend.rs`'s job, covered by its own unit tests).
#[derive(Default)]
pub struct FakeBackend {
    pub calls: Mutex<Vec<&'static str>>,
}

impl FakeBackend {
    fn record(&self, name: &'static str) {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(name);
    }
}

#[async_trait::async_trait]
impl McpBackend for FakeBackend {
    async fn list(&self, _args: ListArgs) -> Result<Vec<TaskRow>, McpError> {
        self.record("list");
        Ok(vec![TaskRow {
            id: Some("01J".to_owned()),
            line: 1,
            raw: "(A) 2026-09-11 Draft +work id:01J".to_owned(),
            priority: Some('A'),
            projects: vec!["work".to_owned()],
            ..TaskRow::default()
        }])
    }
    async fn search(
        &self,
        _t: String,
        _f: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<Vec<TaskRow>, McpError> {
        self.record("search");
        Ok(vec![])
    }
    async fn get(&self, _target: GetTarget) -> Result<TaskRow, McpError> {
        self.record("get");
        Ok(TaskRow::default())
    }
    async fn add(
        &self,
        _text: String,
        _file: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<TaskRow, McpError> {
        self.record("add");
        Ok(TaskRow::default())
    }
    async fn complete(
        &self,
        _id: TaskId,
        _done: bool,
        _w: WorkspaceArg,
    ) -> Result<TaskRow, McpError> {
        self.record("complete");
        Ok(TaskRow::default())
    }
    async fn edit(
        &self,
        _id: TaskId,
        _patch: FieldPatch,
        _w: WorkspaceArg,
    ) -> Result<TaskRow, McpError> {
        self.record("edit");
        Ok(TaskRow::default())
    }
    async fn move_task(
        &self,
        _id: TaskId,
        _anchor: MoveAnchor,
        _w: WorkspaceArg,
    ) -> Result<TaskRow, McpError> {
        self.record("move_task");
        Ok(TaskRow::default())
    }
    async fn delete(&self, _id: TaskId, _confirm: bool, _w: WorkspaceArg) -> Result<(), McpError> {
        self.record("delete");
        Ok(())
    }
    async fn archive(
        &self,
        _file: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<ApplyOutcome, McpError> {
        self.record("archive");
        Ok(ApplyOutcome::default())
    }
    async fn batch(
        &self,
        _ops: Vec<TodoOp>,
        _dry_run: bool,
        _w: WorkspaceArg,
    ) -> Result<ApplyOutcome, McpError> {
        self.record("batch");
        Ok(ApplyOutcome::default())
    }
    async fn history(
        &self,
        _since: Option<Hlc>,
        _id: Option<TaskId>,
        _file: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<Vec<OpSummary>, McpError> {
        self.record("history");
        Ok(vec![])
    }
    async fn raw_read(
        &self,
        _file: RefPath,
        _lines: Vec<u32>,
        _w: WorkspaceArg,
    ) -> Result<Vec<String>, McpError> {
        self.record("raw_read");
        Ok(vec![])
    }
    async fn raw_write(
        &self,
        _file: RefPath,
        _line: u32,
        _text: String,
        _w: WorkspaceArg,
    ) -> Result<(), McpError> {
        self.record("raw_write");
        Ok(())
    }
    async fn notes_get(&self, _id: TaskId, _w: WorkspaceArg) -> Result<String, McpError> {
        self.record("notes_get");
        Ok(String::new())
    }
    async fn notes_set(
        &self,
        _id: TaskId,
        _text: String,
        _w: WorkspaceArg,
    ) -> Result<(), McpError> {
        self.record("notes_set");
        Ok(())
    }
    async fn lint(
        &self,
        _file: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<Vec<LintFinding>, McpError> {
        self.record("lint");
        Ok(vec![LintFinding {
            line: 2,
            finding: "101 chars, over the 100-char hint".to_owned(),
        }])
    }
    async fn conflicts_list(
        &self,
        _file: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<Vec<ConflictFlag>, McpError> {
        self.record("conflicts_list");
        Ok(Vec::new())
    }
    async fn conflicts_resolve(
        &self,
        _id: TaskId,
        _side: ConflictSide,
        _file: Option<RefPath>,
        _w: WorkspaceArg,
    ) -> Result<ApplyOutcome, McpError> {
        self.record("conflicts_resolve");
        Ok(ApplyOutcome::default())
    }
    async fn list_files(&self, _w: WorkspaceArg) -> Result<Vec<FileMeta>, McpError> {
        self.record("list_files");
        Ok(vec![FileMeta {
            path: "todo.txt".to_owned(),
            kind: "todo",
        }])
    }
    async fn get_file(&self, _file: Option<RefPath>, _w: WorkspaceArg) -> Result<String, McpError> {
        self.record("get_file");
        Ok("(A) 2026-09-11 Draft +work id:01J\n".to_owned())
    }
    async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, McpError> {
        self.record("list_workspaces");
        Ok(vec![WorkspaceInfo {
            id: "01J0000000000000000000ABC".to_owned(),
            root: "/workspace".to_owned(),
            added_at_ms: 0,
            root_exists: true,
            has_state: true,
        }])
    }
    fn principal(&self) -> String {
        "user".to_owned()
    }
}
