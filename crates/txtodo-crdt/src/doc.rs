//! The Loro document shape (design §4.2): one movable list per file, one shared `tasks` map, LWW
//! registers per field (ADR 0013). Loro: <https://loro.dev/docs/tutorial/get_started>,
//! movable lists: <https://loro.dev/docs/tutorial/list>, maps: <https://loro.dev/docs/tutorial/map>.

use loro::{
    ExportMode, LoroDoc, LoroEncodeError, LoroError, LoroMap, LoroMovableList, LoroText, LoroValue,
};
use txtodo_model::{FilePath, TaskId, Ulid};

/// The shared map holding every task, keyed by its ULID string.
pub const TASKS_MAP: &str = "tasks";
/// Prefix of each file's movable-list name; the rest is the workspace-relative path.
pub const FILES_PREFIX: &str = "files/";
/// Key of the per-task description text inside the task map.
pub const DESCRIPTION_KEY: &str = "description";
/// A task id whose ULID high byte is `0xFF` is a blank-line sentinel, never a real task: real ULIDs
/// are minted from wall time, so `0xFF` as the leading byte is year 10889.
pub const BLANK_TAG: u128 = 0xFF << 120;

/// True when `id` marks a blank line rather than a task.
pub fn is_blank(id: TaskId) -> bool {
    let bits = id.ulid().to_u128();
    let blank = bits & BLANK_TAG == BLANK_TAG;
    debug_assert_eq!(blank, (bits >> 120) == 0xFF, "the high byte is the tag");
    blank
}

/// The blank-line sentinel id carrying `n` in its low 120 bits. Callers pass a unique `n` (an op id).
pub fn blank_id(n: u128) -> TaskId {
    let id = TaskId::new(Ulid::from_u128(BLANK_TAG | (n & !BLANK_TAG)));
    debug_assert!(is_blank(id));
    id
}

/// The Loro-backed document.
pub struct LoroDocument {
    doc: LoroDoc,
}

impl LoroDocument {
    /// An empty document.
    pub fn open() -> LoroDocument {
        let doc = LoroDoc::new();
        debug_assert_eq!(doc.get_map(TASKS_MAP).len(), 0, "a fresh doc is empty");
        LoroDocument { doc }
    }

    /// The underlying Loro document.
    pub fn doc(&self) -> &LoroDoc {
        &self.doc
    }

    /// The movable list for `path`, created empty on first use.
    pub fn list(&self, path: &FilePath) -> LoroMovableList {
        let name = format!("{FILES_PREFIX}{path}");
        debug_assert!(name.starts_with(FILES_PREFIX));
        self.doc.get_movable_list(name)
    }

    /// The shared tasks map.
    pub fn tasks(&self) -> LoroMap {
        let map = self.doc.get_map(TASKS_MAP);
        debug_assert!(map.is_attached(), "a doc-attached map");
        map
    }

    /// The nested map for `id`, created on first use.
    ///
    /// Ref: <https://docs.rs/loro/1.16.0/loro/struct.LoroMap.html#method.ensure_mergeable_map>
    pub fn task(&self, id: TaskId) -> Result<LoroMap, LoroError> {
        let map = self.tasks().ensure_mergeable_map(&id.to_string())?;
        debug_assert!(map.is_attached());
        Ok(map)
    }

    /// The description text for `id`, created on first use.
    ///
    /// Ref: <https://docs.rs/loro/1.16.0/loro/struct.LoroMap.html#method.ensure_mergeable_text>
    pub fn description(&self, id: TaskId) -> Result<LoroText, LoroError> {
        let text = self.task(id)?.ensure_mergeable_text(DESCRIPTION_KEY)?;
        debug_assert!(text.is_attached());
        Ok(text)
    }

    /// The ids stored in `path`'s list, in order. Blank sentinels are included.
    pub fn ids(&self, path: &FilePath) -> Vec<TaskId> {
        let list = self.list(path);
        let ids: Vec<TaskId> = list
            .to_vec()
            .iter()
            .filter_map(|v| match v {
                LoroValue::String(s) => Ulid::parse(s).map(TaskId::new),
                _ => None,
            })
            .collect();
        debug_assert!(ids.len() <= list.len(), "at most one id per list entry");
        ids
    }

    /// A snapshot of the whole document.
    ///
    /// Ref: <https://docs.rs/loro/1.16.0/loro/struct.LoroDoc.html#method.export>
    pub fn snapshot(&self) -> Result<Vec<u8>, LoroEncodeError> {
        let bytes = self.doc.export(ExportMode::Snapshot)?;
        debug_assert!(!bytes.is_empty(), "a snapshot is never empty");
        Ok(bytes)
    }

    /// Rebuilds a document from a snapshot written by [`LoroDocument::snapshot`].
    pub fn from_snapshot(bytes: &[u8]) -> Result<LoroDocument, LoroError> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        debug_assert!(doc.get_map(TASKS_MAP).is_attached());
        Ok(LoroDocument { doc })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> FilePath {
        FilePath::new(s).expect("test path is valid")
    }

    #[test]
    fn blank_sentinels_are_tagged_and_unique() {
        let a = blank_id(1);
        let b = blank_id(2);
        assert!(is_blank(a) && is_blank(b));
        assert_ne!(a, b);
        // A normal, time-minted id is never blank.
        let real = TaskId::new(Ulid::from_u128(0x0192_0000_0000_0000_0000_0000_0000_0001));
        assert!(!is_blank(real));
    }

    #[test]
    fn lists_are_per_path_and_ids_round_trip() {
        let doc = LoroDocument::open();
        let todo = path("todo.txt");
        let other = path("work/todo.txt");
        let id = TaskId::new(Ulid::from_u128(0x0192_0000_0000_0000_0000_0000_0000_0002));
        doc.list(&todo).insert(0, id.to_string()).unwrap();
        doc.list(&other).insert(0, blank_id(9).to_string()).unwrap();
        assert_eq!(doc.ids(&todo), vec![id]);
        assert_eq!(doc.ids(&other), vec![blank_id(9)]);
        assert_eq!(doc.list(&todo).len(), 1);
    }

    #[test]
    fn snapshot_round_trips_the_whole_document() {
        let doc = LoroDocument::open();
        let todo = path("todo.txt");
        let id = TaskId::new(Ulid::from_u128(0x0192_0000_0000_0000_0000_0000_0000_0003));
        doc.list(&todo).insert(0, id.to_string()).unwrap();
        doc.description(id).unwrap().insert(0, "buy milk").unwrap();
        doc.doc().commit();
        let bytes = doc.snapshot().unwrap();
        let back = LoroDocument::from_snapshot(&bytes).unwrap();
        assert_eq!(back.ids(&todo), vec![id]);
        assert_eq!(back.description(id).unwrap().to_string(), "buy milk");
    }
}
