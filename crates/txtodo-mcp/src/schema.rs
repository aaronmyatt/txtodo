//! [`McpServer`]: the `rmcp` `ServerHandler` — every §6.3 tool via `#[tool_router]`, every §6.4
//! resource/prompt via a manual override (no macro exists for those — see `resources.rs`'s module
//! doc). Every method here is a one-line delegation; the real logic lives in `tools_read.rs`/
//! `tools_write.rs`/`resources.rs`/`prompts.rs`, kept out of this file so the
//! `#[tool_router]`/`#[tool_handler]` macros (which must see the whole `impl` block) stay
//! readable.
//!
//! Every `#[tool]` method also carries the `mcp.call{tool,principal}` span (plan §5,
//! `txtodo-implementation-plan.md:447`; tasks/logging-mcp-call-span): `#[tracing::instrument]`
//! listed *above* `#[tool]`, never below — `#[tool]` rewrites an `async fn` into a sync fn
//! returning a boxed future (see `rmcp-macros`' `tool.rs`), and `tracing::instrument`'s own
//! expansion needs to see the original `async fn` to wrap its body in the span correctly; written
//! the other way round, `#[tool]` would run first and `instrument` would be spanning a plain sync
//! function that merely returns an unstarted future, never entering it. `principal` is always
//! [`crate::backend::McpBackend::principal`]'s non-secret identifier — never a bearer token, tool
//! argument, or task line text (`CLAUDE.md`'s logging rule).

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, GetPromptRequestParams, GetPromptResponse, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool, tool_handler, tool_router};

use crate::backend::{
    AddArgs, ArchiveArgs, BatchArgs, ConflictsListArgs, ConflictsResolveArgs, DeleteArgs, EditArgs,
    GetTarget, HistoryArgs, IdArgs, LintArgs, ListArgs, McpBackend, MoveArgs, NotesGetArgs,
    NotesSetArgs, RawArgs, SearchArgs,
};
use crate::{prompts, resources, tools_read, tools_write};

/// The one MCP server object both transports (`transport.rs`) serve. Schemas here, backed by
/// whichever [`McpBackend`] the caller built — in practice always
/// [`crate::grpc_backend::GrpcMcpBackend`], a gRPC client of `txtodod` (design §6.1: "the daemon
/// is the only thing behind them").
#[derive(Clone)]
pub struct McpServer {
    backend: Arc<dyn McpBackend>,
    tool_router: ToolRouter<McpServer>,
}

impl McpServer {
    /// Wraps `backend` behind every §6.3 tool and §6.4 resource/prompt.
    pub fn new(backend: Arc<dyn McpBackend>) -> McpServer {
        McpServer {
            backend,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl McpServer {
    /// `todo_list`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_list", principal = %self.backend.principal())
    )]
    #[tool(
        description = "List tasks, filtered by an optional query, done (true or false) and file. At most 50 rows per call (limit clamps lower); when more match, a second text block names the offset for the next page."
    )]
    pub async fn todo_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::list(self.backend.as_ref(), args).await
    }

    /// `todo_search`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_search", principal = %self.backend.principal())
    )]
    #[tool(description = "Search tasks by text (case-insensitive substring today).")]
    pub async fn todo_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::search(self.backend.as_ref(), args).await
    }

    /// `todo_get`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_get", principal = %self.backend.principal())
    )]
    #[tool(description = "Get one task by id or line number.")]
    pub async fn todo_get(
        &self,
        Parameters(target): Parameters<GetTarget>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::get(self.backend.as_ref(), target).await
    }

    /// `todo_add`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_add", principal = %self.backend.principal())
    )]
    #[tool(description = "Add a task; text must not carry a leading date or id: tag.")]
    pub async fn todo_add(
        &self,
        Parameters(args): Parameters<AddArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::add(self.backend.as_ref(), args).await
    }

    /// `todo_complete`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_complete", principal = %self.backend.principal())
    )]
    #[tool(
        description = "Mark a task done, preserving its priority as a pri: tag. The done line moves to the bottom of its file, so line numbers change: the id stays valid, and the returned row has the new line."
    )]
    pub async fn todo_complete(
        &self,
        Parameters(IdArgs { id, workspace }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::complete(self.backend.as_ref(), id, true, workspace).await
    }

    /// `todo_uncomplete`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_uncomplete", principal = %self.backend.principal())
    )]
    #[tool(
        description = "Reopen a completed task, restoring its pri: tag as a priority. The line moves up to the end of the open tasks, above the first done line (ids stay valid, line numbers do not)."
    )]
    pub async fn todo_uncomplete(
        &self,
        Parameters(IdArgs { id, workspace }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::complete(self.backend.as_ref(), id, false, workspace).await
    }

    /// `todo_edit`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_edit", principal = %self.backend.principal())
    )]
    #[tool(description = "Field-level patch: priority, due, append, replace.")]
    pub async fn todo_edit(
        &self,
        Parameters(args): Parameters<EditArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::edit(self.backend.as_ref(), args).await
    }

    /// `todo_move`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_move", principal = %self.backend.principal())
    )]
    #[tool(
        description = "Reorder a task immediately before or after another task in the same file."
    )]
    pub async fn todo_move(
        &self,
        Parameters(args): Parameters<MoveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::move_task(self.backend.as_ref(), args).await
    }

    /// `todo_lint`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_lint", principal = %self.backend.principal())
    )]
    #[tool(
        description = "Report parse quirks and file hygiene of a document, as `txtodo lint` does. Read-only."
    )]
    pub async fn todo_lint(
        &self,
        Parameters(args): Parameters<LintArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::lint(self.backend.as_ref(), args).await
    }

    /// `todo_conflicts_list`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_conflicts_list", principal = %self.backend.principal())
    )]
    #[tool(
        description = "List open merge-conflict flags (two devices rewrote the same task): the task, both texts."
    )]
    pub async fn todo_conflicts_list(
        &self,
        Parameters(args): Parameters<ConflictsListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::conflicts_list(self.backend.as_ref(), args).await
    }

    /// `todo_conflicts_resolve`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_conflicts_resolve", principal = %self.backend.principal())
    )]
    #[tool(
        description = "Resolve one conflicted task keeping mine, theirs or merged (the file as it is). Attributed to the agent."
    )]
    pub async fn todo_conflicts_resolve(
        &self,
        Parameters(args): Parameters<ConflictsResolveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::conflicts_resolve(self.backend.as_ref(), args).await
    }

    /// `todo_delete`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_delete", principal = %self.backend.principal())
    )]
    #[tool(description = "Delete a task; confirm must be true.")]
    pub async fn todo_delete(
        &self,
        Parameters(args): Parameters<DeleteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::delete(self.backend.as_ref(), args).await
    }

    /// `todo_archive`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_archive", principal = %self.backend.principal())
    )]
    #[tool(description = "Move completed tasks in file to the bottom, same file.")]
    pub async fn todo_archive(
        &self,
        Parameters(args): Parameters<ArchiveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::archive(self.backend.as_ref(), args).await
    }

    /// `todo_batch`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_batch", principal = %self.backend.principal())
    )]
    #[tool(
        description = "Apply several operations in order. dry_run writes nothing and returns the unified diff the batch would make; the real run then applies the same plan, one commit per file. A batch naming one task twice cannot be previewed (split it), and todo_move cannot be previewed yet."
    )]
    pub async fn todo_batch(
        &self,
        Parameters(args): Parameters<BatchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::batch(self.backend.as_ref(), args).await
    }

    /// `todo_history`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_history", principal = %self.backend.principal())
    )]
    #[tool(description = "Read the op log, optionally since an HLC or for one task/file.")]
    pub async fn todo_history(
        &self,
        Parameters(args): Parameters<HistoryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::history(self.backend.as_ref(), args).await
    }

    /// `todo_raw`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_raw", principal = %self.backend.principal())
    )]
    #[tool(description = "Line-level read (lines[]) or write (line+text); needs the raw scope.")]
    pub async fn todo_raw(
        &self,
        Parameters(args): Parameters<RawArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::raw(self.backend.as_ref(), args).await
    }

    /// `todo_notes_get`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_notes_get", principal = %self.backend.principal())
    )]
    #[tool(description = "Read a task's notes.md.")]
    pub async fn todo_notes_get(
        &self,
        Parameters(NotesGetArgs { id, workspace }): Parameters<NotesGetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::notes_get(self.backend.as_ref(), id, workspace).await
    }

    /// `todo_notes_set`.
    #[tracing::instrument(
        name = "mcp.call",
        skip_all,
        fields(tool = "todo_notes_set", principal = %self.backend.principal())
    )]
    #[tool(description = "Replace a task's notes.md.")]
    pub async fn todo_notes_set(
        &self,
        Parameters(NotesSetArgs {
            id,
            text,
            workspace,
        }): Parameters<NotesSetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::notes_set(self.backend.as_ref(), id, text, workspace).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        resources::list(self.backend.as_ref()).await
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(resources::list_templates())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        Ok(resources::read(self.backend.as_ref(), &request.uri)
            .await?
            .into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(prompts::list())
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let args = request.arguments.clone();
        let lookup = move |k: &str| {
            args.as_ref()
                .and_then(|m| m.get(k))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        };
        Ok(prompts::get(self.backend.as_ref(), &request.name, lookup)
            .await?
            .into())
    }
}
