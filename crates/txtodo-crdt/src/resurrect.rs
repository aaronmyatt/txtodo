//! Delete-vs-edit / delete-vs-complete conflict resolution (`specs/conflicts.md` rows 6-7, plan
//! M4 `crdt-conflict-table`). `Deleted`, `Completed` and the description text are independent CRDT
//! registers (`crate::lww`, `crate::to_loro`) that merge independently on `LoroDocument::import`:
//! nothing links them, so a concurrent delete on one device and an edit or completion on another
//! would otherwise leave the task deleted with the other side's change buried underneath it. The
//! policy is fixed, not configurable (`tasks/crdt-conflict-table/notes.md`, "As built"): a
//! concurrent delete always loses to a concurrent edit or completion.
//!
//! Concurrency, like `crate::review`, comes from `Imported`'s frontiers, never the HLC (an `Hlc`
//! is a total order and cannot say whether two writes saw each other): `mine` is
//! `diff(ancestor, before)`, `theirs` is `diff(ancestor, remote)`, both expressed against the
//! point the two views last agreed on, so only genuinely concurrent changes are ever compared.

use std::collections::HashMap;

use loro::event::Diff;
use loro::{Frontiers, LoroMap, LoroResult, TextDelta};
use txtodo_model::{Field, FieldValue, Hlc, TaskId};

use crate::doc::sync::Imported;
use crate::doc::{LoroDocument, decode_field_value, encode_field_value, field_from_key, field_key};
use crate::lww::{Lww, write_if_newer};

/// What one side changed about a task, relative to the ancestor both sides last agreed on.
#[derive(Default)]
struct TaskChange {
    /// `Deleted` was set to `true`.
    deleted: bool,
    /// `Completed` was set to `true`.
    completed: bool,
    /// The description text changed (a real insert/delete, not a no-op retain).
    edited: bool,
}

/// Forces `Deleted` back to `false` wherever a concurrent delete lost to a concurrent edit or
/// completion. Runs after every [`LoroDocument::import`] so both sides reach the resolution
/// independently: no coordination between devices is needed, since a resolved register is always
/// `false`, and Loro's own causal order (this write commits strictly after the import it is
/// correcting) makes it win regardless of how the two devices' own corrections tie-break against
/// each other.
pub(crate) fn resolve(doc: &mut LoroDocument, imported: &Imported) -> LoroResult<()> {
    if !imported.applied {
        return Ok(());
    }
    let mine = changes(doc, &imported.ancestor, &imported.before)?;
    let theirs = changes(doc, &imported.ancestor, &imported.remote)?;
    let mut resurrected = false;
    for (task, mine_change) in &mine {
        let Some(their_change) = theirs.get(task) else {
            continue;
        };
        let loses =
            delete_loses(mine_change, their_change) || delete_loses(their_change, mine_change);
        if loses && doc.is_deleted(*task) {
            clear_deleted(doc, *task)?;
            resurrected = true;
        }
    }
    if resurrected {
        doc.commit();
    }
    Ok(())
}

/// True when `deleter`'s concurrent delete must lose to `other`'s concurrent edit or completion.
fn delete_loses(deleter: &TaskChange, other: &TaskChange) -> bool {
    deleter.deleted && (other.completed || other.edited)
}

/// Per-task field/text changes in `doc.diff(from, to)`, keyed by task.
fn changes(
    doc: &LoroDocument,
    from: &Frontiers,
    to: &Frontiers,
) -> LoroResult<HashMap<TaskId, TaskChange>> {
    let batch = doc.diff(from, to)?;
    let mut out: HashMap<TaskId, TaskChange> = HashMap::new();
    // Bounded by the number of containers the diff touched.
    for (cid, diff) in batch.iter() {
        let Some(task) = doc.task_of_container(cid) else {
            continue;
        };
        match diff {
            Diff::Map(delta) => note_field_change(out.entry(task).or_default(), delta),
            Diff::Text(deltas) if is_real_edit(deltas) => {
                out.entry(task).or_default().edited = true;
            }
            _ => {}
        }
    }
    Ok(out)
}

/// True for a text delta that actually inserts or deletes, not a bare retain.
fn is_real_edit(deltas: &[TextDelta]) -> bool {
    deltas
        .iter()
        .any(|d| !matches!(d, TextDelta::Retain { .. }))
}

/// Records a `Deleted`/`Completed` flip to `true` from one side's map delta. Malformed or
/// unrelated entries are skipped, the same leniency `LoroDocument::is_deleted` uses — this is a
/// read-side heuristic, not the authoritative op translation `from_loro` does.
fn note_field_change(change: &mut TaskChange, delta: &loro::event::MapDelta<'_>) {
    for (key, new) in &delta.updated {
        let Some(field) = field_from_key(key) else {
            continue;
        };
        let Some(value) = new.as_ref().and_then(|voc| voc.as_value()) else {
            continue;
        };
        let Some(fv) = Lww::decode(value).and_then(|lww| decode_field_value(field, &lww.value))
        else {
            continue;
        };
        match (field, fv) {
            (Field::Deleted, FieldValue::Bool(true)) => change.deleted = true,
            (Field::Completed, FieldValue::Bool(true)) => change.completed = true,
            _ => {}
        }
    }
}

/// Writes `Deleted = false`, keyed on the register's own current stamp so the write always lands
/// (`Lww::wins_over`'s tie case) without fabricating a new `Hlc`.
fn clear_deleted(doc: &LoroDocument, task: TaskId) -> LoroResult<()> {
    let Some(map) = doc.task_map_if_exists(task) else {
        return Ok(());
    };
    let Some(hlc) = current_hlc(&map, Field::Deleted) else {
        return Ok(());
    };
    write_if_newer(
        &map,
        field_key(Field::Deleted),
        encode_field_value(FieldValue::Bool(false)),
        hlc,
    )?;
    Ok(())
}

/// The stamp on a task map's field register, if it is present and well-formed.
fn current_hlc(map: &LoroMap, field: Field) -> Option<Hlc> {
    let value = map.get(field_key(field))?.into_value().ok()?;
    Lww::decode(&value).map(|lww| lww.hlc)
}
