//! Loro document shape (design §4.2): one `LoroMovableList` per file keyed `files/<path>`,
//! plus one shared `LoroMap` named `tasks` whose values are per-task nested `LoroMap`s.
//!
//! Within a task map the `description` key is a `LoroText`; every other field is an LWW register
//! (see [`crate::lww`]). Blank lines are reserved-prefix sentinel [`TaskId`]s (top byte `0xFF`)
//! with no `tasks` entry, so one ordered list keeps a blank run ordered through a merge. Loro
//! list API: <https://loro.dev/docs/tutorial/list> · map: <https://loro.dev/docs/tutorial/map>.

use loro::{
    Container, ContainerID, ContainerTrait, ExportMode, LoroDoc, LoroMap, LoroMovableList,
    LoroResult, LoroText, LoroValue, ValueOrContainer,
};
use std::collections::HashMap;

use txtodo_core::{Date, Priority, Ulid};
use txtodo_model::{Field, FieldValue, FilePath, Op, TaskId};

use crate::from_loro::FromLoroError;
use crate::lww::Lww;
use crate::to_loro::{ToLoroError, apply};

mod shadow;
pub(crate) mod sync;
mod view;

/// Root map name holding one nested `LoroMap` per task.
pub(crate) const TASKS_MAP: &str = "tasks";
/// Prefix of every per-file movable list's root name.
pub(crate) const FILES_PREFIX: &str = "files/";
/// Key of the description `LoroText` inside each task map.
pub(crate) const DESCRIPTION_KEY: &str = "description";
/// Reserved top byte: any task id whose high byte is `0xFF` is a blank-line sentinel.
pub(crate) const BLANK_PREFIX_MASK: u128 = 0xFF00_0000_0000_0000_0000_0000_0000_0000;

/// The Loro-backed document. In-memory only: the `txtodo-store` op log is the source of truth
/// and this doc is re-hydrated from an op iterator on start (design §4.2 / ADR 0013).
pub struct LoroDocument {
    /// The underlying Loro document.
    doc: LoroDoc,
    /// Low bits minted into the next blank sentinel; deterministic per re-hydration.
    next_blank: u128,
    /// Per-file-list shadow of the ids in order (see `doc/shadow.rs`), keyed by list name.
    shadows: HashMap<String, Vec<TaskId>>,
}

impl LoroDocument {
    /// Opens an empty document.
    pub fn open() -> LoroDocument {
        LoroDocument {
            doc: LoroDoc::new(),
            next_blank: 0,
            shadows: HashMap::new(),
        }
    }

    /// Exports the current state as a full snapshot. See <https://docs.rs/loro> `ExportMode`.
    pub fn snapshot(&self) -> Result<Vec<u8>, loro::LoroEncodeError> {
        self.doc.export(ExportMode::Snapshot)
    }

    /// Rebuilds a document from a snapshot written by [`LoroDocument::snapshot`].
    pub fn from_snapshot(bytes: &[u8]) -> LoroResult<LoroDocument> {
        let doc = LoroDoc::from_snapshot(bytes)?;
        let next_blank = Self::blank_after(&doc);
        let mut loaded = LoroDocument {
            doc,
            next_blank,
            shadows: HashMap::new(),
        };
        // The lists came from bytes, not from our mutations: shadows rebuild on first use.
        loaded.invalidate_shadows();
        Ok(loaded)
    }

    /// Opens an empty document and applies each op through [`apply`], one commit per op.
    pub fn hydrate(ops: impl Iterator<Item = Op>) -> Result<LoroDocument, ToLoroError> {
        let mut doc = LoroDocument::open();
        for op in ops {
            apply(&mut doc, &op)?;
        }
        Ok(doc)
    }

    /// The current state frontiers, for [`LoroDocument::diff`].
    pub fn state_frontiers(&self) -> loro::Frontiers {
        self.doc.state_frontiers()
    }

    /// The owned diff between two frontiers, for [`crate::from_batch`].
    pub fn diff(
        &self,
        a: &loro::Frontiers,
        b: &loro::Frontiers,
    ) -> LoroResult<loro::event::DiffBatch> {
        self.doc.diff(a, b)
    }

    /// Commits the pending Loro transaction, making its diffs observable.
    pub(crate) fn commit(&self) {
        self.doc.commit();
    }

    /// The per-file movable list, creating the root container if needed.
    pub(crate) fn file_list(&self, file: &FilePath) -> LoroMovableList {
        self.doc.get_movable_list(file_list_name(file))
    }

    /// The shared `tasks` root map.
    pub(crate) fn tasks_map(&self) -> LoroMap {
        self.doc.get_map(TASKS_MAP)
    }

    /// The task's nested map, creating it (mergeably) if needed.
    pub(crate) fn task_map(&self, task: TaskId) -> LoroResult<LoroMap> {
        self.tasks_map().ensure_mergeable_map(&task_id_str(task))
    }

    /// The task's nested map if it already exists.
    pub(crate) fn task_map_if_exists(&self, task: TaskId) -> Option<LoroMap> {
        match self.tasks_map().get(&task_id_str(task)) {
            Some(ValueOrContainer::Container(Container::Map(m))) => Some(m),
            _ => None,
        }
    }

    /// The task's description text, creating the task map (mergeably) if needed.
    pub(crate) fn description_text(&self, task: TaskId) -> LoroResult<LoroText> {
        self.task_map(task)?.ensure_mergeable_text(DESCRIPTION_KEY)
    }

    /// The task's description text if it already exists.
    pub(crate) fn description_if_exists(&self, task: TaskId) -> Option<LoroText> {
        match self.task_map_if_exists(task)?.get(DESCRIPTION_KEY) {
            Some(ValueOrContainer::Container(Container::Text(t))) => Some(t),
            _ => None,
        }
    }

    /// Mints the next blank sentinel id. Deterministic per re-hydration (see `next_blank`).
    pub(crate) fn blank_id(&mut self) -> TaskId {
        let bits = BLANK_PREFIX_MASK | self.next_blank;
        debug_assert!(
            self.next_blank < BLANK_PREFIX_MASK,
            "blank sentinel counter stays below the reserved prefix"
        );
        self.next_blank = self.next_blank.saturating_add(1);
        TaskId::new(Ulid::from_u128(bits))
    }

    /// Every `files/<path>` root list's path, in root-map key order.
    pub(crate) fn file_paths(&self) -> Vec<FilePath> {
        let LoroValue::Map(root) = self.doc.get_deep_value() else {
            return Vec::new();
        };
        root.iter()
            .filter_map(|(k, v)| {
                let path = k.strip_prefix(FILES_PREFIX)?;
                if !matches!(v, LoroValue::List(_)) {
                    return None;
                }
                FilePath::new(path).ok()
            })
            .collect()
    }

    /// The file whose list currently holds `task`, if any.
    pub(crate) fn file_of_task(&self, task: TaskId) -> Option<FilePath> {
        self.file_paths()
            .into_iter()
            .find(|p| self.index_in(p, task).is_some())
    }

    /// The file list whose container id is `id`, if any.
    pub(crate) fn file_of_container(&self, id: &ContainerID) -> Option<FilePath> {
        self.file_paths()
            .into_iter()
            .find(|p| &self.file_list(p).id() == id)
    }

    /// The task whose nested map or description text has container id `id`, if any.
    pub(crate) fn task_of_container(&self, id: &ContainerID) -> Option<TaskId> {
        for key in self.tasks_map().keys() {
            let Some(task) = parse_task_id(&key) else {
                continue;
            };
            let Some(map) = self.task_map_if_exists(task) else {
                continue;
            };
            if &map.id() == id {
                return Some(task);
            }
            if let Some(text) = self.description_if_exists(task)
                && &text.id() == id
            {
                return Some(task);
            }
        }
        None
    }

    /// One past the largest blank sentinel low bits found in a snapshot, so new sentinels never
    /// collide with the ones already written there.
    fn blank_after(doc: &LoroDoc) -> u128 {
        let LoroValue::Map(root) = doc.get_deep_value() else {
            return 0;
        };
        let mut max = 0u128;
        for (k, v) in root.iter() {
            if !k.starts_with(FILES_PREFIX) {
                continue;
            }
            let LoroValue::List(list) = v else {
                continue;
            };
            for item in list.iter() {
                let LoroValue::String(s) = item else {
                    continue;
                };
                let Some(id) = Ulid::parse(s.as_ref()) else {
                    continue;
                };
                let bits = id.to_u128();
                if bits & BLANK_PREFIX_MASK == BLANK_PREFIX_MASK {
                    max = max.max(bits & !BLANK_PREFIX_MASK);
                }
            }
        }
        max.saturating_add(1)
    }
}

/// The root name of a file's movable list.
pub(crate) fn file_list_name(file: &FilePath) -> String {
    format!("{FILES_PREFIX}{file}")
}

/// The list-entry string for a task id (its ULID text).
pub(crate) fn task_id_str(task: TaskId) -> String {
    task.ulid().to_string()
}

/// Parses a list-entry string back to a task id.
pub(crate) fn parse_task_id(s: &str) -> Option<TaskId> {
    Ulid::parse(s).map(TaskId::new)
}

/// True when the task id carries the reserved blank prefix.
pub fn is_blank(task: TaskId) -> bool {
    task.ulid().to_u128() & BLANK_PREFIX_MASK == BLANK_PREFIX_MASK
}

/// The Loro map key for a prefix field.
pub(crate) fn field_key(field: Field) -> &'static str {
    match field {
        Field::Completed => "completed",
        Field::CompletionDate => "completion_date",
        Field::CreationDate => "creation_date",
        Field::Priority => "priority",
        Field::Deleted => "deleted",
        Field::Quirks => "quirks",
    }
}

/// The prefix field named by a Loro map key.
pub(crate) fn field_from_key(key: &str) -> Option<Field> {
    match key {
        "completed" => Some(Field::Completed),
        "completion_date" => Some(Field::CompletionDate),
        "creation_date" => Some(Field::CreationDate),
        "priority" => Some(Field::Priority),
        "deleted" => Some(Field::Deleted),
        "quirks" => Some(Field::Quirks),
        _ => None,
    }
}

/// Encodes a field value as a plain `LoroValue` (the `v` half of an LWW register).
pub(crate) fn encode_field_value(fv: FieldValue) -> LoroValue {
    match fv {
        FieldValue::Bool(b) => LoroValue::Bool(b),
        FieldValue::Date(None) => LoroValue::Null,
        FieldValue::Date(Some((y, m, d))) => LoroValue::from(format!("{y:04}-{m:02}-{d:02}")),
        FieldValue::Priority(None) => LoroValue::Null,
        FieldValue::Priority(Some(c)) => LoroValue::from(c.to_string()),
        FieldValue::Quirks(bits) => LoroValue::I64(i64::from(bits)),
    }
}

/// Decodes the `v` half of an LWW register back to a field value, checked against `field`.
pub(crate) fn decode_field_value(field: Field, v: &LoroValue) -> Option<FieldValue> {
    match field {
        Field::Completed | Field::Deleted => v.as_bool().copied().map(FieldValue::Bool),
        Field::CompletionDate | Field::CreationDate => {
            if v.is_null() {
                return Some(FieldValue::Date(None));
            }
            let d = Date::parse(v.as_string()?.as_ref())?;
            Some(FieldValue::Date(Some((d.year(), d.month(), d.day()))))
        }
        Field::Priority => {
            if v.is_null() {
                return Some(FieldValue::Priority(None));
            }
            let c = v.as_string()?.as_ref().chars().next()?;
            let p = Priority::new(c)?;
            Some(FieldValue::Priority(Some(p.as_char())))
        }
        Field::Quirks => v
            .as_i64()
            .copied()
            .and_then(|i| u16::try_from(i).ok())
            .map(FieldValue::Quirks),
    }
}

/// The list index of a task id, if present. Slow path (a Loro walk) used only before a shadow
/// exists; `LoroDocument::index_in` is the fast path.
pub(crate) fn index_of(list: &LoroMovableList, task: TaskId) -> Option<usize> {
    position_of(list, &task_id_str(task))
}

/// A linear scan by `get(i)` — no `to_vec`, so a 10k-entry list is not cloned per lookup.
fn position_of(list: &LoroMovableList, needle: &str) -> Option<usize> {
    let len = list.len();
    // Bounded by the list length.
    (0..len).find(|&i| {
        list.get(i)
            .and_then(|voc| voc.into_value().ok())
            .is_some_and(|v| v.as_string().is_some_and(|s| s.as_str() == needle))
    })
}

/// Rebuilds a canonical `Insert` line from the task map's description text and prefix fields.
/// Canonical, not byte-faithful: quirks (tabs, trailing space) are flag bits and are not replayed.
pub fn rebuild_line(doc: &LoroDocument, task: TaskId) -> Result<String, FromLoroError> {
    let map = doc
        .task_map_if_exists(task)
        .ok_or(FromLoroError::MissingTask(task))?;
    let description = doc
        .description_if_exists(task)
        .map_or(String::new(), |t| t.to_string());
    let completed = match read_field(&map, Field::Completed)? {
        Some(FieldValue::Bool(b)) => b,
        Some(_) => return Err(FromLoroError::Malformed("completed is not a bool".into())),
        None => false,
    };
    let completion_date = read_date(&map, Field::CompletionDate)?;
    let creation_date = read_date(&map, Field::CreationDate)?;
    let priority = match read_field(&map, Field::Priority)? {
        Some(FieldValue::Priority(p)) => p,
        Some(_) => return Err(FromLoroError::Malformed("priority is not a char".into())),
        None => None,
    };
    let prefix = txtodo_core::Prefix {
        completed,
        completion_date: completion_date.and_then(|(y, m, d)| Date::new(y, m, d)),
        creation_date: creation_date.and_then(|(y, m, d)| Date::new(y, m, d)),
        priority: priority.and_then(Priority::new),
    };
    let has_description = !description.is_empty();
    Ok(format!(
        "{}{}",
        txtodo_core::emit_prefix(&prefix, has_description),
        description
    ))
}

/// Reads a date field back as `Option<(year, month, day)>`.
fn read_date(map: &LoroMap, field: Field) -> Result<Option<(u16, u8, u8)>, FromLoroError> {
    match read_field(map, field)? {
        Some(FieldValue::Date(d)) => Ok(d),
        Some(_) => Err(FromLoroError::Malformed("date field is not a date".into())),
        None => Ok(None),
    }
}

/// Reads and decodes one LWW register from a task map.
fn read_field(map: &LoroMap, field: Field) -> Result<Option<FieldValue>, FromLoroError> {
    let Some(voc) = map.get(field_key(field)) else {
        return Ok(None);
    };
    let value = voc
        .into_value()
        .ok()
        .ok_or_else(|| FromLoroError::Malformed(format!("field {field:?} is not a value")))?;
    let lww = Lww::decode(&value).ok_or_else(|| {
        FromLoroError::Malformed(format!("field {field:?} is not an LWW register"))
    })?;
    decode_field_value(field, &lww.value)
        .map(Some)
        .ok_or_else(|| FromLoroError::Malformed(format!("field {field:?} has a bad value")))
}
