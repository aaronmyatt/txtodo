//! Tool call bodies: one function per `#[tool]` in `schema.rs`, so that impl block (which the
//! `#[tool_router]`/`#[tool_handler]` macros must see whole) stays a thin, readable list of
//! one-line delegations. Read-only tools are in `tools_read.rs`, mutating ones in `tools_write.rs`.

use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

/// Every tool returns one JSON text block — a single, uniform content shape across the whole
/// table, so a client never has to branch on which tool it called to parse the result.
pub(crate) fn json_result<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string(value)
        .map_err(|e| ErrorData::internal_error(format!("could not encode result: {e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}
