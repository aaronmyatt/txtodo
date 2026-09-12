//! Loro diff data -> `Vec<Op>`, exhaustive over container kinds, stamped with the local [`Hlc`]
//! and [`Principal`]. Input is the owned `loro::event::DiffBatch` from `LoroDoc::diff`; task/file
//! ownership is resolved by scanning the document, since `DiffBatch` carries no `path`. The
//! list/text/map mapping is exhaustive. Loro diff API: <https://docs.rs/loro>.

use loro::{
    ContainerID, TextDelta, ValueOrContainer, event::Diff, event::DiffBatch, event::ListDiffItem,
};
use txtodo_model::{FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, TextEdit};

use crate::doc::{
    DESCRIPTION_KEY, LoroDocument, decode_field_value, field_from_key, is_blank, parse_task_id,
};
use crate::lww::Lww;

mod list;

use list::pair_list;

/// The local stamp to put on every op translated from a diff.
#[derive(Debug, Clone)]
pub struct Stamp {
    /// The already-ticked HLC for this batch.
    pub hlc: Hlc,
    /// Who produced the change.
    pub principal: Principal,
}

/// Why a Loro diff could not be translated into ops.
#[derive(Debug)]
pub enum FromLoroError {
    /// A task had no file list entry to stamp as the op's file.
    MissingFile(TaskId),
    /// A task's map was expected but missing.
    MissingTask(TaskId),
    /// A diff carried a value this document shape cannot read.
    Malformed(String),
    /// A container kind or diff shape this first cut does not map yet.
    Unsupported(&'static str),
    /// The underlying Loro read failed.
    Loro(loro::LoroError),
}

impl std::fmt::Display for FromLoroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FromLoroError::MissingFile(t) => write!(f, "no file list holds task {t}"),
            FromLoroError::MissingTask(t) => write!(f, "no task map for task {t}"),
            FromLoroError::Malformed(what) => write!(f, "malformed diff: {what}"),
            FromLoroError::Unsupported(what) => write!(f, "unsupported diff: {what}"),
            FromLoroError::Loro(e) => write!(f, "loro: {e}"),
        }
    }
}

impl std::error::Error for FromLoroError {}

impl From<loro::LoroError> for FromLoroError {
    fn from(e: loro::LoroError) -> FromLoroError {
        FromLoroError::Loro(e)
    }
}

/// Translates one owned diff batch into ops, in the order the batch carries.
pub fn from_batch(
    doc: &LoroDocument,
    batch: &DiffBatch,
    stamp: &Stamp,
    mint: &mut dyn FnMut() -> OpId,
) -> Result<Vec<Op>, FromLoroError> {
    let diffs = batch
        .iter()
        .map(|(cid, d)| (cid.clone(), d.clone()))
        .collect();
    convert(doc, diffs, stamp, mint)
}

fn convert(
    doc: &LoroDocument,
    diffs: Vec<(ContainerID, Diff<'static>)>,
    stamp: &Stamp,
    mint: &mut dyn FnMut() -> OpId,
) -> Result<Vec<Op>, FromLoroError> {
    let mut ops = Vec::new();
    let mut ctx = Ctx {
        doc,
        stamp,
        mint,
        ops: &mut ops,
    };
    let mut events = ListEvents::default();
    collect_list(ctx.doc, &diffs, &mut events)?;
    pair_list(&mut ctx, &events)?;
    map_and_text(&mut ctx, &diffs, &events.inserted)?;
    Ok(ops)
}

#[derive(Default)]
struct ListEvents {
    inserts: Vec<InsertEvent>,
    moved: Vec<MovedEvent>,
    deletes: Vec<DeleteEvent>,
    inserted: Vec<TaskId>,
}

struct InsertEvent {
    task: TaskId,
    file: FilePath,
    pos: usize,
    blank: bool,
}

struct MovedEvent {
    task: TaskId,
    file: FilePath,
    pos: usize,
}

struct DeleteEvent {
    file: FilePath,
    pos: usize,
}

struct Ctx<'a> {
    doc: &'a LoroDocument,
    stamp: &'a Stamp,
    mint: &'a mut dyn FnMut() -> OpId,
    ops: &'a mut Vec<Op>,
}

fn collect_list(
    doc: &LoroDocument,
    diffs: &[(ContainerID, Diff<'static>)],
    events: &mut ListEvents,
) -> Result<(), FromLoroError> {
    for (cid, diff) in diffs {
        let Some(items) = diff.as_list() else {
            continue;
        };
        let Some(file) = doc.file_of_container(cid) else {
            continue;
        };
        let mut pos = 0usize;
        for item in items {
            collect_list_item(events, &file, item, &mut pos)?;
        }
    }
    Ok(())
}

fn collect_list_item(
    events: &mut ListEvents,
    file: &FilePath,
    item: &ListDiffItem,
    pos: &mut usize,
) -> Result<(), FromLoroError> {
    match item {
        ListDiffItem::Insert { insert, is_move } => {
            collect_insert(events, file, insert, *is_move, pos)
        }
        ListDiffItem::Delete { .. } => {
            events.deletes.push(DeleteEvent {
                file: file.clone(),
                pos: *pos,
            });
            Ok(())
        }
        ListDiffItem::Retain { retain } => {
            *pos += retain;
            Ok(())
        }
    }
}

fn collect_insert(
    events: &mut ListEvents,
    file: &FilePath,
    insert: &[ValueOrContainer],
    is_move: bool,
    pos: &mut usize,
) -> Result<(), FromLoroError> {
    for v in insert {
        let s = value_str(v)
            .ok_or_else(|| FromLoroError::Malformed("list insert is not a string".into()))?;
        let task =
            parse_task_id(s).ok_or_else(|| FromLoroError::Malformed(format!("bad task id {s}")))?;
        if is_move {
            events.moved.push(MovedEvent {
                task,
                file: file.clone(),
                pos: *pos,
            });
        } else if is_blank(task) {
            events.inserts.push(InsertEvent {
                task,
                file: file.clone(),
                pos: *pos,
                blank: true,
            });
        } else {
            events.inserted.push(task);
            events.inserts.push(InsertEvent {
                task,
                file: file.clone(),
                pos: *pos,
                blank: false,
            });
        }
        *pos += 1;
    }
    Ok(())
}

fn map_and_text(
    ctx: &mut Ctx,
    diffs: &[(ContainerID, Diff<'static>)],
    inserted: &[TaskId],
) -> Result<(), FromLoroError> {
    for (cid, diff) in diffs {
        match diff {
            Diff::Map(delta) => map_diff(ctx, cid, delta, inserted)?,
            Diff::Text(deltas) => text_diff(ctx, cid, deltas, inserted)?,
            Diff::List(_) => {}
            Diff::Tree(_) | Diff::Counter(_) | Diff::Unknown => {
                return Err(FromLoroError::Unsupported(
                    "tree/counter/unknown containers are not in this document",
                ));
            }
        }
    }
    Ok(())
}

fn map_diff(
    ctx: &mut Ctx,
    cid: &ContainerID,
    delta: &loro::event::MapDelta<'_>,
    inserted: &[TaskId],
) -> Result<(), FromLoroError> {
    let Some(task) = ctx.doc.task_of_container(cid) else {
        return Ok(());
    };
    if inserted.contains(&task) {
        return Ok(());
    }
    let file = ctx
        .doc
        .file_of_task(task)
        .ok_or(FromLoroError::MissingFile(task))?;
    for (key, new) in &delta.updated {
        if key.as_ref() == DESCRIPTION_KEY {
            continue;
        }
        let field = field_from_key(key).ok_or_else(|| FromLoroError::Malformed(key.to_string()))?;
        let Some(voc) = new else {
            continue;
        };
        let value = voc
            .as_value()
            .ok_or_else(|| FromLoroError::Malformed(format!("field {key} is not a value")))?;
        let lww = Lww::decode(value).ok_or_else(|| {
            FromLoroError::Malformed(format!("field {key} is not an LWW register"))
        })?;
        let fv = decode_field_value(field, &lww.value)
            .ok_or_else(|| FromLoroError::Malformed(format!("field {key} has a bad value")))?;
        push(
            ctx,
            file.clone(),
            OpKind::SetField {
                task,
                field,
                value: fv,
            },
        );
    }
    Ok(())
}

fn text_diff(
    ctx: &mut Ctx,
    cid: &ContainerID,
    deltas: &[TextDelta],
    inserted: &[TaskId],
) -> Result<(), FromLoroError> {
    let Some(task) = ctx.doc.task_of_container(cid) else {
        return Ok(());
    };
    if inserted.contains(&task) {
        return Ok(());
    }
    let file = ctx
        .doc
        .file_of_task(task)
        .ok_or(FromLoroError::MissingFile(task))?;
    let edits = text_edits(deltas);
    push(ctx, file, OpKind::EditText { task, edits });
    Ok(())
}

fn text_edits(deltas: &[TextDelta]) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut pos = 0usize;
    for d in deltas {
        match d {
            TextDelta::Retain { retain, .. } => pos += retain,
            TextDelta::Insert { insert, .. } => {
                edits.push(TextEdit::Insert {
                    at: pos,
                    text: insert.clone(),
                });
                pos += insert.chars().count();
            }
            TextDelta::Delete { delete } => edits.push(TextEdit::Delete {
                at: pos,
                len: *delete,
            }),
        }
    }
    edits
}

fn predecessor(doc: &LoroDocument, file: &FilePath, pos: usize) -> Option<TaskId> {
    let values = doc.file_list(file).to_vec();
    let slice = values.get(..pos)?;
    slice
        .iter()
        .rev()
        .filter_map(|v| v.as_string().and_then(|s| parse_task_id(s.as_ref())))
        .find(|t| !is_blank(*t))
}

fn push(ctx: &mut Ctx, file: FilePath, kind: OpKind) {
    ctx.ops.push(make_op(ctx.stamp, &mut *ctx.mint, file, kind));
}

fn make_op(stamp: &Stamp, mint: &mut dyn FnMut() -> OpId, file: FilePath, kind: OpKind) -> Op {
    Op {
        id: mint(),
        hlc: stamp.hlc,
        principal: stamp.principal.clone(),
        file,
        kind,
    }
}

fn value_str(v: &ValueOrContainer) -> Option<&str> {
    v.as_value()?.as_string().map(|s| s.as_ref())
}
