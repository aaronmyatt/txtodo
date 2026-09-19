//! `todo_move`: a same-file reorder next to another task (design §6.3; task `mcp-move-reorder`).
//! `before` maps to the daemon's `MoveBefore` mutation; `after` to `MoveBefore` the task that
//! follows the anchor, or `MoveToEnd` when the anchor is last. Split out of `grpc_write.rs` for
//! its file budget.

use txtodo_proto::v1 as pb;

use crate::backend::{MoveAnchor, TaskId, TaskRow, WorkspaceArg};
use crate::error::McpError;
use crate::grpc_read::{get_file_text, locate_by_id};
use crate::grpc_write::{GrpcCtx, apply_one};
use crate::parse;

fn task_ref(line: u32, id: &str) -> pb::TaskRef {
    pb::TaskRef {
        line_number: line,
        task_id: id.to_owned(),
    }
}

/// The mutation that puts task `id` (on `line`) next to `anchor`, or `None` when it already sits
/// there. Pure: reads only `text`, the document as the daemon holds it.
pub(crate) fn move_mutation(
    text: &str,
    id: &str,
    line: u32,
    anchor: &MoveAnchor,
) -> Result<Option<pb::mutation::Kind>, McpError> {
    let (MoveAnchor::Before(anchor_id) | MoveAnchor::After(anchor_id)) = anchor;
    if anchor_id == id {
        return Err(McpError::invalid_params(
            "a task cannot be moved next to itself",
        ));
    }
    let (anchor_line, _) = parse::find_by_id(text, anchor_id)
        .ok_or_else(|| McpError::not_found(format!("anchor {anchor_id} is not in this file")))?;
    let task = Some(task_ref(line, id));
    match anchor {
        MoveAnchor::Before(_) => Ok(Some(pb::mutation::Kind::MoveBefore(pb::MoveBefore {
            task,
            before: Some(task_ref(anchor_line, anchor_id)),
        }))),
        MoveAnchor::After(_) => {
            let next = parse::lines(text)
                .into_iter()
                .find(|(n, raw)| *n > anchor_line && !raw.trim().is_empty());
            let Some((n, raw)) = next else {
                return Ok(Some(pb::mutation::Kind::MoveToEnd(pb::MoveToEnd { task })));
            };
            let next_id = parse::parse_row(n, raw).id.unwrap_or_default();
            if next_id == id {
                return Ok(None); // already right after the anchor
            }
            Ok(Some(pb::mutation::Kind::MoveBefore(pb::MoveBefore {
                task,
                before: Some(task_ref(n, &next_id)),
            })))
        }
    }
}

/// `todo_move`.
pub async fn move_task(
    ctx: GrpcCtx,
    id: TaskId,
    anchor: MoveAnchor,
    workspace: WorkspaceArg,
) -> Result<TaskRow, McpError> {
    let (path, line, _row) = locate_by_id(ctx.client.clone(), &id, workspace.clone()).await?;
    let text = get_file_text(ctx.client.clone(), &path, workspace.clone()).await?;
    let client = ctx.client.clone();
    if let Some(kind) = move_mutation(&text, &id, line, &anchor)? {
        let mutation = pb::Mutation { kind: Some(kind) };
        apply_one(ctx, &path, mutation, workspace.clone()).await?;
    }
    let (_, _, row) = locate_by_id(client, &id, workspace).await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a id:01\nb id:02\n\nc id:03\n";

    fn kind(id: &str, line: u32, anchor: MoveAnchor) -> Option<pb::mutation::Kind> {
        move_mutation(TEXT, id, line, &anchor).unwrap_or_else(|e| panic!("{e:?}"))
    }

    #[test]
    fn before_maps_to_move_before_with_both_lines() {
        let Some(pb::mutation::Kind::MoveBefore(m)) =
            kind("03", 4, MoveAnchor::Before("01".into()))
        else {
            panic!("expected MoveBefore");
        };
        assert_eq!(m.task, Some(task_ref(4, "03")));
        assert_eq!(m.before, Some(task_ref(1, "01")));
    }

    #[test]
    fn after_moves_before_the_next_task_and_skips_blank_lines() {
        // After 02 the next non-blank line is 03 (the blank between is ignored).
        let Some(pb::mutation::Kind::MoveBefore(m)) = kind("01", 1, MoveAnchor::After("02".into()))
        else {
            panic!("expected MoveBefore");
        };
        assert_eq!(m.before, Some(task_ref(4, "03")));
    }

    #[test]
    fn after_the_last_task_is_move_to_end() {
        assert!(matches!(
            kind("01", 1, MoveAnchor::After("03".into())),
            Some(pb::mutation::Kind::MoveToEnd(_))
        ));
    }

    #[test]
    fn already_right_after_the_anchor_is_a_no_op() {
        assert!(kind("02", 2, MoveAnchor::After("01".into())).is_none());
    }

    #[test]
    fn a_missing_anchor_or_itself_is_refused() {
        assert!(move_mutation(TEXT, "03", 4, &MoveAnchor::Before("99".into())).is_err());
        assert!(move_mutation(TEXT, "03", 4, &MoveAnchor::Before("03".into())).is_err());
    }
}
