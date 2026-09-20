//! What `GetFile` answers: a document's bytes, their hash, and one task id per line (task
//! sidecar-task-ids). Moved out of `handle.rs` to keep that file within its line budget, the same
//! way `conflict_row.rs` was.

use crate::expected::Hash;
use txtodo_model::TaskId;

/// A document's current bytes and hash, plus the id of each line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contents {
    /// Exact projection bytes.
    pub bytes: Vec<u8>,
    /// blake3 of `bytes`.
    pub hash: Hash,
    /// One entry per line of `bytes`, in file order; `None` for a blank line. Read in the same
    /// actor turn as `bytes`, so entry `i` always describes line `i`. Under Sidecar identity this
    /// is the only place a client can learn a line's id: the text carries no `id:` tag.
    pub task_ids: Vec<Option<TaskId>>,
}

impl Contents {
    /// `task_ids` as the wire wants them: ULID text, `""` for a blank line
    /// (`FileContents.task_ids`).
    pub fn task_id_texts(&self) -> Vec<String> {
        self.task_ids
            .iter()
            .map(|id| id.map(|t| t.to_string()).unwrap_or_default())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::task_id;

    #[test]
    fn a_blank_line_is_an_empty_string_and_a_task_is_its_ulid() {
        let c = Contents {
            bytes: b"a\n\n".to_vec(),
            hash: crate::actor::hash_of(b"a\n\n"),
            task_ids: vec![Some(task_id(7)), None],
        };
        let texts = c.task_id_texts();
        assert_eq!(texts.len(), 2);
        assert_eq!(texts[0], task_id(7).to_string());
        assert_eq!(texts[0].len(), 26, "ULID text is 26 chars");
        assert_eq!(texts[1], "");
    }
}
