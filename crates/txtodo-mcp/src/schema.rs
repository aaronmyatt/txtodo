//! [`McpServer`]: the `rmcp` `ServerHandler` — every §6.3 tool via `#[tool_router]`, every §6.4
//! resource/prompt via a manual override (no macro exists for those — see `resources.rs`'s module
//! doc). Every method here is a one-line delegation; the real logic lives in `tools_read.rs`/
//! `tools_write.rs`/`resources.rs`/`prompts.rs`, kept out of this file so the
//! `#[tool_router]`/`#[tool_handler]` macros (which must see the whole `impl` block) stay
//! readable.

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
    AddArgs, ArchiveArgs, BatchArgs, DeleteArgs, EditArgs, GetTarget, HistoryArgs, IdArgs,
    ListArgs, McpBackend, MoveArgs, NotesGetArgs, NotesSetArgs, RawArgs, SearchArgs,
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
    #[tool(description = "List tasks, filtered by an optional query and file, capped at limit.")]
    pub async fn todo_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::list(self.backend.as_ref(), args).await
    }

    /// `todo_search`.
    #[tool(description = "Search tasks by text (case-insensitive substring today).")]
    pub async fn todo_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::search(self.backend.as_ref(), args).await
    }

    /// `todo_get`.
    #[tool(description = "Get one task by id or line number.")]
    pub async fn todo_get(
        &self,
        Parameters(target): Parameters<GetTarget>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::get(self.backend.as_ref(), target).await
    }

    /// `todo_add`.
    #[tool(description = "Add a task; text must not carry a leading date or id: tag.")]
    pub async fn todo_add(
        &self,
        Parameters(args): Parameters<AddArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::add(self.backend.as_ref(), args).await
    }

    /// `todo_complete`.
    #[tool(description = "Mark a task done, preserving its priority as a pri: tag.")]
    pub async fn todo_complete(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::complete(self.backend.as_ref(), id, true).await
    }

    /// `todo_uncomplete`.
    #[tool(description = "Reopen a completed task, restoring its pri: tag as a priority.")]
    pub async fn todo_uncomplete(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::complete(self.backend.as_ref(), id, false).await
    }

    /// `todo_edit`.
    #[tool(description = "Field-level patch: priority, due, append, replace.")]
    pub async fn todo_edit(
        &self,
        Parameters(args): Parameters<EditArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::edit(self.backend.as_ref(), args).await
    }

    /// `todo_move`.
    #[tool(
        description = "Reorder a task before/after another (same-file; see the As-built notes for the current limitation)."
    )]
    pub async fn todo_move(
        &self,
        Parameters(args): Parameters<MoveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::move_task(self.backend.as_ref(), args).await
    }

    /// `todo_delete`.
    #[tool(description = "Delete a task; confirm must be true.")]
    pub async fn todo_delete(
        &self,
        Parameters(args): Parameters<DeleteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::delete(self.backend.as_ref(), args).await
    }

    /// `todo_archive`.
    #[tool(description = "Move completed tasks in file to done.txt.")]
    pub async fn todo_archive(
        &self,
        Parameters(args): Parameters<ArchiveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::archive(self.backend.as_ref(), args).await
    }

    /// `todo_batch`.
    #[tool(
        description = "Apply several operations in order; dry_run skips execution (no diff yet)."
    )]
    pub async fn todo_batch(
        &self,
        Parameters(args): Parameters<BatchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::batch(self.backend.as_ref(), args).await
    }

    /// `todo_history`.
    #[tool(description = "Read the op log, optionally since an HLC or for one task/file.")]
    pub async fn todo_history(
        &self,
        Parameters(args): Parameters<HistoryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::history(self.backend.as_ref(), args).await
    }

    /// `todo_raw`.
    #[tool(description = "Line-level read (lines[]) or write (line+text); needs the raw scope.")]
    pub async fn todo_raw(
        &self,
        Parameters(args): Parameters<RawArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::raw(self.backend.as_ref(), args).await
    }

    /// `todo_notes_get`.
    #[tool(description = "Read a task's notes.md.")]
    pub async fn todo_notes_get(
        &self,
        Parameters(NotesGetArgs { id }): Parameters<NotesGetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_read::notes_get(self.backend.as_ref(), id).await
    }

    /// `todo_notes_set`.
    #[tool(description = "Replace a task's notes.md.")]
    pub async fn todo_notes_set(
        &self,
        Parameters(NotesSetArgs { id, text }): Parameters<NotesSetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        tools_write::notes_set(self.backend.as_ref(), id, text).await
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
