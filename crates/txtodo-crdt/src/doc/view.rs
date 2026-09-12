//! Read-side views of a [`LoroDocument`] for a host that keeps its own byte-faithful lines beside
//! the doc (the daemon's `DocState`, plan M4): the ordered ids of a file list, the newest blank
//! sentinel, and a deep copy. Split from `doc.rs` for the file budget.

use loro::LoroResult;
use txtodo_model::{Field, FieldValue, FilePath, TaskId};

use super::{BLANK_PREFIX_MASK, LoroDocument, read_field};

impl LoroDocument {
    /// A deep, independent copy. `LoroDoc::clone` shares state; `fork` does not
    /// (<https://docs.rs/loro/latest/loro/struct.LoroDoc.html#method.fork>), so a host can apply
    /// to the copy and discard it on error, leaving the original untouched.
    pub fn fork(&self) -> LoroDocument {
        let forked = LoroDocument {
            doc: self.doc.fork(),
            next_blank: self.next_blank,
            shadows: self.shadows.clone(),
        };
        debug_assert_eq!(forked.next_blank, self.next_blank);
        debug_assert_eq!(
            forked.doc.get_deep_value(),
            self.doc.get_deep_value(),
            "a fork starts equal"
        );
        forked
    }

    /// The ids in `file`'s list, in order, blank sentinels included. Entries that are not task-id
    /// strings (there should be none) are skipped rather than trusted.
    pub fn list_ids(&self, file: &FilePath) -> Vec<TaskId> {
        let ids = self.ids_in(file);
        debug_assert_eq!(
            ids.len(),
            self.file_list(file).len(),
            "every entry is an id"
        );
        debug_assert!(ids.iter().all(|id| self.index_in(file, *id).is_some()));
        ids
    }

    /// Whether the task's `deleted` register is set. A missing task or field reads as not deleted.
    pub fn is_deleted(&self, task: TaskId) -> bool {
        let Some(map) = self.task_map_if_exists(task) else {
            return false;
        };
        let deleted = matches!(
            read_field(&map, Field::Deleted),
            Ok(Some(FieldValue::Bool(true)))
        );
        debug_assert!(
            !super::is_blank(task) || !deleted,
            "sentinels are never deleted"
        );
        deleted
    }

    /// The task's description text as Loro holds it, if the task exists.
    pub fn description(&self, task: TaskId) -> Option<String> {
        self.description_if_exists(task).map(|t| t.to_string())
    }

    /// Replaces the description text wholesale and commits. For a host whose byte-faithful line
    /// changed its description outside an `EditText` (e.g. `complete` appending ` pri:B`), so the
    /// text and the host's bytes never drift apart.
    pub fn set_description(&mut self, task: TaskId, text: &str) -> LoroResult<()> {
        let t = self.description_text(task)?;
        let stale = t.len_unicode();
        if stale > 0 {
            t.delete(0, stale)?;
        }
        t.insert(0, text)?;
        self.commit();
        debug_assert_eq!(t.to_string(), text);
        debug_assert_eq!(self.description(task).as_deref(), Some(text));
        Ok(())
    }

    /// A read-only copy of the document as it was at `frontiers` (for "mine"/"theirs" text).
    /// <https://docs.rs/loro/latest/loro/struct.LoroDoc.html#method.fork_at>
    pub fn at(&self, frontiers: &loro::Frontiers) -> LoroResult<LoroDocument> {
        let doc = self.doc.fork_at(frontiers)?;
        let view = LoroDocument {
            doc,
            next_blank: self.next_blank,
            shadows: std::collections::HashMap::new(),
        };
        debug_assert_eq!(view.next_blank, self.next_blank);
        debug_assert!(view.shadows.is_empty(), "a view rebuilds shadows lazily");
        Ok(view)
    }

    /// The blank sentinel minted most recently by a `BlankInsert`, if any.
    pub fn last_blank_id(&self) -> Option<TaskId> {
        if self.next_blank == 0 {
            return None;
        }
        let bits = BLANK_PREFIX_MASK | (self.next_blank - 1);
        debug_assert!(self.next_blank - 1 < BLANK_PREFIX_MASK);
        let id = TaskId::new(txtodo_core::Ulid::from_u128(bits));
        debug_assert!(super::is_blank(id));
        Some(id)
    }
}
