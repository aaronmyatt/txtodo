//! The list half of the diff → op mapping: one list diff batch becomes exactly one `Op`.
//! A child module of `from_loro`, so it shares `Ctx`, `push`, `predecessor` and the event structs
//! without widening them. Loro list events:
//! <https://docs.rs/loro/1.16.0/loro/event/enum.ListDiffItem.html>

use crate::doc::rebuild_line;
use txtodo_model::{FilePath, OpKind, TaskId};

use super::{Ctx, FromLoroError, ListEvents, predecessor, push};

/// Pairs one list diff batch into a single op. Anything that is not a clean single-op shape is
/// rejected rather than guessed at.
pub(super) fn pair_list(ctx: &mut Ctx, events: &ListEvents) -> Result<(), FromLoroError> {
    match (
        events.inserts.len(),
        events.moved.len(),
        events.deletes.len(),
    ) {
        (0, 0, 0) => {}
        (1, 0, 0) => {
            let ins = &events.inserts[0];
            let after = predecessor(ctx.doc, &ins.file, ins.pos);
            if ins.blank {
                push(ctx, ins.file.clone(), OpKind::BlankInsert { after });
            } else {
                let line = rebuild_line(ctx.doc, ins.task)?;
                push(
                    ctx,
                    ins.file.clone(),
                    OpKind::Insert {
                        task: ins.task,
                        after,
                        line,
                    },
                );
            }
        }
        (0, 1, 1) => {
            let m = &events.moved[0];
            push_move(ctx, events, m.task, &m.file, m.pos)?;
        }
        (1, 0, 1) if !events.inserts[0].blank => {
            let ins = &events.inserts[0];
            push_move(ctx, events, ins.task, &ins.file, ins.pos)?;
        }
        (0, 0, 1) => {
            let d = &events.deletes[0];
            let after = predecessor(ctx.doc, &d.file, d.pos);
            push(ctx, d.file.clone(), OpKind::BlankRemove { after });
        }
        _ => {
            return Err(FromLoroError::Unsupported(
                "list diff batch does not map to exactly one op",
            ));
        }
    }
    Ok(())
}

/// The `Move` shape: one delete plus one insert, or one native move.
fn push_move(
    ctx: &mut Ctx,
    events: &ListEvents,
    task: TaskId,
    file: &FilePath,
    pos: usize,
) -> Result<(), FromLoroError> {
    let after = predecessor(ctx.doc, file, pos);
    push(
        ctx,
        events.deletes[0].file.clone(),
        OpKind::Move {
            task,
            after,
            to_file: file.clone(),
        },
    );
    Ok(())
}
