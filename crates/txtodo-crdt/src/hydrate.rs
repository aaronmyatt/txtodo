//! Bulk hydration of one file list from parsed lines, for a host opening a document (the daemon's
//! `DocState::from_file`). Appends in order under one commit, so a 10k-line file costs O(n)
//! instead of the O(n²) that per-op `Insert { after }` anchoring would (each anchor is a list
//! scan). Loro list API: <https://loro.dev/docs/tutorial/list>.

use txtodo_model::{FilePath, Hlc, TaskId};

use crate::doc::{LoroDocument, task_id_str};
use crate::to_loro::{ToLoroError, populate};

/// One line to hydrate, in file order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HydrateLine<'a> {
    /// A task line (already carrying its `id:`), with its text minus the line ending.
    Task {
        /// The id in the line.
        task: TaskId,
        /// The line text.
        line: &'a str,
    },
    /// An empty line.
    Blank,
}

/// Appends `lines` to `file`'s list, which must be empty, and returns the list ids in order
/// (task ids as given, fresh blank sentinels for blanks) so the host can key its bytes by them.
pub fn hydrate_file(
    doc: &mut LoroDocument,
    file: &FilePath,
    lines: &[HydrateLine<'_>],
    hlc: Hlc,
) -> Result<Vec<TaskId>, ToLoroError> {
    let list = doc.file_list(file);
    if !list.is_empty() {
        return Err(ToLoroError::Unsupported(
            "hydrate_file needs an empty file list",
        ));
    }
    let mut ids = Vec::with_capacity(lines.len());
    // Bounded by lines.len(); the host caps that at its MAX_LINES_PER_FILE.
    for entry in lines {
        let id = match *entry {
            HydrateLine::Task { task, line } => {
                list.push(task_id_str(task))?;
                populate(doc, task, line, hlc)?;
                task
            }
            HydrateLine::Blank => {
                let sentinel = doc.blank_id();
                list.push(task_id_str(sentinel))?;
                sentinel
            }
        };
        ids.push(id);
    }
    doc.commit();
    debug_assert_eq!(ids.len(), lines.len());
    debug_assert_eq!(list.len(), lines.len(), "every line became one entry");
    Ok(ids)
}
