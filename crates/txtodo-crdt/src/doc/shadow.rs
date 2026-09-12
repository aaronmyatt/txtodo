//! A per-file shadow of each Loro movable list: the same ids, in the same order, as a plain
//! `Vec<TaskId>`. Loro's list is addressed by index and only walkable with `get(i)` (≈ µs each in
//! debug), so keying by task id meant an O(n) walk per op and O(n²) per batch — 168 s for a
//! 10k-line adopt (measured 2026-09-12). Every list mutation this crate performs goes through the
//! methods here so the shadow never drifts; anything that changes a list behind our back (a
//! snapshot load, a future import) calls [`LoroDocument::invalidate_shadows`] and the shadow is
//! rebuilt from the list on next use.

use loro::LoroResult;
use txtodo_model::{FilePath, TaskId};

use super::{LoroDocument, file_list_name, parse_task_id, task_id_str};

impl LoroDocument {
    /// Builds the shadow for `file` from the Loro list if it is not there yet.
    pub(crate) fn ensure_shadow(&mut self, file: &FilePath) {
        let name = file_list_name(file);
        if self.shadows.contains_key(&name) {
            return;
        }
        let ids: Vec<TaskId> = self
            .file_list(file)
            .to_vec()
            .iter()
            .filter_map(|v| v.as_string().and_then(|s| parse_task_id(s.as_ref())))
            .collect();
        debug_assert_eq!(
            ids.len(),
            self.file_list(file).len(),
            "every entry is an id"
        );
        self.shadows.insert(name, ids);
        debug_assert!(self.shadows.contains_key(&file_list_name(file)));
    }

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

    /// The id at `idx` in `file`'s list, if any.
    pub(crate) fn id_at(&self, file: &FilePath, idx: usize) -> Option<TaskId> {
        match self.shadows.get(&file_list_name(file)) {
            Some(ids) => ids.get(idx).copied(),
            None => self.ids_in(file).get(idx).copied(),
        }
    }

    /// The list length.
    pub(crate) fn len_of(&self, file: &FilePath) -> usize {
        match self.shadows.get(&file_list_name(file)) {
            Some(ids) => ids.len(),
            None => self.file_list(file).len(),
        }
    }

    /// Inserts `task` at `idx` in the list and the shadow.
    pub(crate) fn list_insert(
        &mut self,
        file: &FilePath,
        idx: usize,
        task: TaskId,
    ) -> LoroResult<()> {
        self.ensure_shadow(file);
        self.file_list(file).insert(idx, task_id_str(task))?;
        let shadow = self.shadow_mut(file);
        debug_assert!(idx <= shadow.len());
        shadow.insert(idx, task);
        debug_assert_eq!(shadow.get(idx), Some(&task));
        Ok(())
    }

    /// Appends `task` to the list and the shadow.
    pub(crate) fn list_push(&mut self, file: &FilePath, task: TaskId) -> LoroResult<()> {
        self.ensure_shadow(file);
        self.file_list(file).push(task_id_str(task))?;
        let shadow = self.shadow_mut(file);
        shadow.push(task);
        debug_assert_eq!(shadow.last(), Some(&task));
        debug_assert!(!shadow.is_empty());
        Ok(())
    }

    /// Deletes the entry at `idx` from the list and the shadow.
    pub(crate) fn list_delete(&mut self, file: &FilePath, idx: usize) -> LoroResult<()> {
        self.ensure_shadow(file);
        self.file_list(file).delete(idx, 1)?;
        let shadow = self.shadow_mut(file);
        debug_assert!(idx < shadow.len());
        let before = shadow.len();
        shadow.remove(idx);
        debug_assert_eq!(shadow.len() + 1, before);
        Ok(())
    }

    /// Moves the entry at `from` to `to` (final index) in the list and the shadow.
    pub(crate) fn list_mov(&mut self, file: &FilePath, from: usize, to: usize) -> LoroResult<()> {
        self.ensure_shadow(file);
        self.file_list(file).mov(from, to)?;
        let shadow = self.shadow_mut(file);
        debug_assert!(from < shadow.len() && to < shadow.len());
        let id = shadow.remove(from);
        shadow.insert(to, id);
        debug_assert_eq!(shadow.get(to), Some(&id));
        Ok(())
    }

    fn shadow_mut(&mut self, file: &FilePath) -> &mut Vec<TaskId> {
        let name = file_list_name(file);
        debug_assert!(self.shadows.contains_key(&name), "ensure_shadow ran first");
        self.shadows.entry(name).or_default()
    }

    /// Test-only check that a shadow still equals its list.
    #[cfg(test)]
    pub(crate) fn shadow_matches_list(&self, file: &FilePath) -> bool {
        let from_list: Vec<TaskId> = self
            .file_list(file)
            .to_vec()
            .iter()
            .filter_map(|v| v.as_string().and_then(|s| parse_task_id(s.as_ref())))
            .collect();
        self.shadows
            .get(&file_list_name(file))
            .is_none_or(|ids| *ids == from_list)
    }
}
