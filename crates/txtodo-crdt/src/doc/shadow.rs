//! A per-file shadow of each Loro movable list: the same ids, in the same order, as a plain
//! `Vec<TaskId>`. Loro's list is addressed by index and only walkable with `get(i)` (≈ µs each in
//! debug), so keying by task id meant an O(n) walk per op and O(n²) per batch — 168 s for a
//! 10k-line adopt (measured 2026-09-12). Every list mutation this crate performs goes through the
//! methods here so the shadow never drifts; anything that changes a list behind our back (a
//! snapshot load, a future import) calls [`LoroDocument::invalidate_shadows`] and the shadow is
//! rebuilt from the list on next use.

use txtodo_model::{FilePath, TaskId};

use super::{LoroDocument, file_list_name, parse_task_id};

impl LoroDocument {
    /// Forgets every shadow; the next use rebuilds from the lists.
    pub(crate) fn invalidate_shadows(&mut self) {
        self.shadows.clear();
        debug_assert!(self.shadows.is_empty());
    }

    /// The ids of `file`'s list in order — from the shadow when built, else from the list.
    pub(crate) fn ids_in(&self, file: &FilePath) -> Vec<TaskId> {
        if let Some(ids) = self.shadows.get(&file_list_name(file)) {
            return ids.clone();
        }
        self.file_list(file)
            .to_vec()
            .iter()
            .filter_map(|v| v.as_string().and_then(|s| parse_task_id(s.as_ref())))
            .collect()
    }

    /// The index of `task` in `file`'s list, if present.
    pub(crate) fn index_in(&self, file: &FilePath, task: TaskId) -> Option<usize> {
        match self.shadows.get(&file_list_name(file)) {
            Some(ids) => ids.iter().position(|id| *id == task),
            None => super::index_of(&self.file_list(file), task),
        }
    }
}
