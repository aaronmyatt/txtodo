//! Read-side views of a [`LoroDocument`] for a host that keeps its own byte-faithful lines beside
//! the doc (the daemon's `DocState`, plan M4): the ordered ids of a file list, the newest blank
//! sentinel, and a deep copy. Split from `doc.rs` for the file budget.

use loro::LoroValue;
use txtodo_model::{FilePath, TaskId};

use super::{BLANK_PREFIX_MASK, LoroDocument, parse_task_id};

impl LoroDocument {
    /// A deep, independent copy. `LoroDoc::clone` shares state; `fork` does not
    /// (<https://docs.rs/loro/latest/loro/struct.LoroDoc.html#method.fork>), so a host can apply
    /// to the copy and discard it on error, leaving the original untouched.
    pub fn fork(&self) -> LoroDocument {
        let forked = LoroDocument {
            doc: self.doc.fork(),
            next_blank: self.next_blank,
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
        let values = self.file_list(file).to_vec();
        let ids: Vec<TaskId> = values
            .iter()
            .filter_map(|v| match v {
                LoroValue::String(s) => parse_task_id(s.as_ref()),
                _ => None,
            })
            .collect();
        debug_assert!(ids.len() <= values.len());
        debug_assert_eq!(ids.len(), values.len(), "every list entry is a task id");
        ids
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
