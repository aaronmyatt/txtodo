//! Bodies for the read-only tools (`todo_list`, `todo_search`, `todo_get`, `todo_history`,
//! `todo_raw` read mode). Called one-line-each from `schema.rs`'s `#[tool_router]` impl.

use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::backend::{
    ConflictsListArgs, GetTarget, HistoryArgs, LintArgs, ListArgs, McpBackend, RawArgs, SearchArgs,
    TaskRow, WorkspaceArg,
};
use crate::error::McpError;
use crate::tools::json_result;

/// The most rows one `todo_list` call returns (task payload-budget): a row is the raw line
/// (at most `doc::MAX_ROW_BYTES`) plus its parsed fields, so 50 rows stay under 512 KiB.
pub const MAX_LIST_ROWS: usize = 50;

/// `todo_list`: the filtered rows, paged. The first content block is the JSON array every client
/// already parses; a second text block appears only when rows were left out, naming the
/// `offset` for the next call.
pub async fn list(backend: &dyn McpBackend, args: ListArgs) -> Result<CallToolResult, ErrorData> {
    let (limit, offset) = (args.limit, args.offset);
    let rows = backend
        .list(ListArgs {
            limit: None,
            offset: None,
            ..args
        })
        .await?;
    let (page, more) = page_rows(rows, limit, offset);
    let mut result = json_result(&page)?;
    if let Some(note) = more {
        result.content.push(ContentBlock::text(note));
    }
    Ok(result)
}

/// The pure half of [`list`]: the page `[offset, offset + min(limit, MAX_LIST_ROWS))` of `rows`,
/// and the note for the client when rows remain past it.
pub(crate) fn page_rows(
    mut rows: Vec<TaskRow>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> (Vec<TaskRow>, Option<String>) {
    let total = rows.len();
    let page = limit
        .filter(|&l| l > 0)
        .map_or(MAX_LIST_ROWS, |l| (l as usize).min(MAX_LIST_ROWS));
    let start = (offset.unwrap_or(0) as usize).min(total);
    let end = (start + page).min(total);
    let note = (end < total).then(|| {
        format!(
            "{} of {total} matching rows shown (rows {}-{end}); call todo_list again with \
             offset {end} for the rest",
            end - start,
            start + 1
        )
    });
    rows.truncate(end);
    (rows.split_off(start), note)
}

/// `todo_search`.
pub async fn search(
    backend: &dyn McpBackend,
    args: SearchArgs,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.search(args.text, args.file, args.workspace).await?)
}

/// `todo_get`.
pub async fn get(backend: &dyn McpBackend, target: GetTarget) -> Result<CallToolResult, ErrorData> {
    if target.id.is_none() && target.line.is_none() {
        return Err(McpError::invalid_params("todo_get needs id or line").into());
    }
    json_result(&backend.get(target).await?)
}

/// `todo_lint`: read-only findings, the same a `txtodo lint` prints.
pub async fn lint(backend: &dyn McpBackend, args: LintArgs) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.lint(args.file, args.workspace).await?)
}

/// `todo_conflicts_list`.
pub async fn conflicts_list(
    backend: &dyn McpBackend,
    args: ConflictsListArgs,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.conflicts_list(args.file, args.workspace).await?)
}

/// `todo_history`.
pub async fn history(
    backend: &dyn McpBackend,
    args: HistoryArgs,
) -> Result<CallToolResult, ErrorData> {
    let ops = backend
        .history(args.since, args.id, args.file, args.workspace)
        .await?;
    json_result(&ops)
}

/// `todo_raw`, both modes: `text` present means write, otherwise read (`args.rs`'s doc).
pub async fn raw(backend: &dyn McpBackend, args: RawArgs) -> Result<CallToolResult, ErrorData> {
    let RawArgs {
        file,
        lines,
        line,
        text,
        workspace,
    } = args;
    match text {
        Some(text) => {
            let line =
                line.ok_or_else(|| McpError::invalid_params("todo_raw write mode needs line"))?;
            backend.raw_write(file, line, text, workspace).await?;
            json_result(&())
        }
        None => {
            let lines =
                lines.ok_or_else(|| McpError::invalid_params("todo_raw read mode needs lines"))?;
            json_result(&backend.raw_read(file, lines, workspace).await?)
        }
    }
}

/// `todo_notes_get`.
pub async fn notes_get(
    backend: &dyn McpBackend,
    id: String,
    workspace: WorkspaceArg,
) -> Result<CallToolResult, ErrorData> {
    json_result(&backend.notes_get(id, workspace).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<TaskRow> {
        (1..=n)
            .map(|i| TaskRow {
                line: i as u32,
                raw: format!("task {i}"),
                ..TaskRow::default()
            })
            .collect()
    }

    #[test]
    fn a_short_list_is_returned_whole_with_no_note() {
        let (page, note) = page_rows(rows(3), None, None);
        assert_eq!(page.len(), 3);
        assert_eq!(note, None);
    }

    #[test]
    fn a_long_list_is_capped_at_fifty_and_says_so() {
        let (page, note) = page_rows(rows(120), None, None);
        assert_eq!(page.len(), MAX_LIST_ROWS);
        assert_eq!(page[0].line, 1);
        let note = note.unwrap_or_default();
        assert!(note.contains("50 of 120"), "{note}");
        assert!(note.contains("offset 50"), "{note}");
    }

    #[test]
    fn offset_pages_and_a_larger_limit_is_clamped() {
        let (page, note) = page_rows(rows(120), Some(500), Some(100));
        assert_eq!(page.len(), 20, "the last page is short");
        assert_eq!(page[0].line, 101);
        assert_eq!(note, None, "nothing left past the last page");
        let (page, _) = page_rows(rows(120), Some(10), Some(5));
        assert_eq!(page.first().map(|r| r.line), Some(6));
        assert_eq!(page.len(), 10);
        let (page, _) = page_rows(rows(3), None, Some(99));
        assert!(
            page.is_empty(),
            "an offset past the end is empty, not an error"
        );
    }
}
