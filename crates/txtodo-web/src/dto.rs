//! The JSON the desktop frontend reads, field for field as `apps/desktop/src-tauri/src/dto.rs`
//! (and `dto_duplicates.rs`) send it, so the same Svelte code runs on this host. Hashes cross as
//! lowercase hex. Ref: <https://serde.rs/derive.html>

use serde::Serialize;
use txtodo_engine::duplicates::DuplicateGroup;
use txtodo_model::{Op, OpKind, TaskId};
use txtodo_store::{Stored, kind_tag};

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The inverse of [`hex`]; anything but an even run of hex digits is no bytes at all, which no
/// document hash matches, so a `Replace` on it is refused as stale rather than guessed at.
/// Ref: <https://doc.rust-lang.org/std/primitive.u8.html#method.from_str_radix>
pub fn unhex(text: &str) -> Vec<u8> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return Vec::new();
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .unwrap_or_default()
}

/// One list (`list_files`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileInfoDto {
    /// Workspace-relative path.
    pub path: String,
    /// Hex blake3 of the list.
    pub hash: String,
    /// `"FILE_KIND_TODO"`: this host holds lists only.
    pub kind: String,
    /// Completed task lines.
    pub done: u32,
    /// Task lines.
    pub total: u32,
}

/// One list's text (`get_file`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileContentsDto {
    /// Workspace-relative path.
    pub path: String,
    /// The list's text.
    pub text: String,
    /// Hex blake3 of the bytes.
    pub hash: String,
    /// One per line: the task id (ULID text), `""` for a blank.
    pub task_ids: Vec<String>,
}

/// One op (`history`, a change event).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpSummaryDto {
    /// Position in the log.
    pub seq: i64,
    /// ULID text.
    pub op_id: String,
    /// The device that made it, ULID text.
    pub device: String,
    /// `you@<device>` / `agent:<name>@<device>` / `external@<device>`.
    pub principal: String,
    /// The store's kind tag: insert, set_field, edit_text, ...
    pub kind: String,
    /// The task it touches, `""` for a blank-line op.
    pub task_id: String,
    /// One line for a human. Here the kind tag: the daemon's summary text is not ported yet.
    pub summary: String,
}

impl OpSummaryDto {
    /// `op` at `seq`.
    pub fn of(seq: i64, op: &Op) -> OpSummaryDto {
        let kind = kind_tag(&op.kind).to_owned();
        OpSummaryDto {
            seq,
            op_id: op.id.ulid().to_string(),
            device: op.hlc.device.to_string(),
            principal: op.principal.to_string(),
            kind: kind.clone(),
            task_id: task_of(&op.kind).map(|t| t.to_string()).unwrap_or_default(),
            summary: kind,
        }
    }

    /// A stored op.
    pub fn stored(s: &Stored) -> OpSummaryDto {
        OpSummaryDto::of(s.seq.0, &s.op)
    }
}

/// The task an op touches.
pub fn task_of(kind: &OpKind) -> Option<TaskId> {
    match kind {
        OpKind::Insert { task, .. }
        | OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. }
        | OpKind::Move { task, .. }
        | OpKind::RemoveTag { task, .. } => Some(*task),
        OpKind::NotesEdit { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. } => None,
    }
}

/// One change, sent to the page as a `daemon-change` event.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChangeDto {
    /// The list that changed.
    pub path: String,
    /// Hex blake3 after.
    pub hash: String,
    /// The ops it appended.
    pub ops: Vec<OpSummaryDto>,
    /// The list's duplicate groups after (ADR 0032).
    pub duplicate_groups: u32,
}

/// `apply`'s result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApplyResultDto {
    /// Ops appended.
    pub applied: u32,
    /// Hex blake3 after.
    pub hash: String,
    /// The batch stamp's wall clock.
    pub hlc_wall_ms: u64,
    /// The batch stamp's counter.
    pub hlc_counter: u32,
}

/// `history`'s result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HistoryDto {
    /// Ops, newest first.
    pub ops: Vec<OpSummaryDto>,
}

/// One copy in a duplicate group (`list_duplicates`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DuplicateTaskDto {
    /// ULID text.
    pub task_id: String,
    /// 1-based line number.
    pub line_number: u32,
}

/// Lines with the same text (ADR 0032).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DuplicateGroupDto {
    /// The copies, in file order.
    pub tasks: Vec<DuplicateTaskDto>,
}

impl From<&DuplicateGroup> for DuplicateGroupDto {
    fn from(g: &DuplicateGroup) -> DuplicateGroupDto {
        DuplicateGroupDto {
            tasks: g
                .tasks
                .iter()
                .map(|(task, line)| DuplicateTaskDto {
                    task_id: task.to_string(),
                    line_number: u32::try_from(*line).unwrap_or(u32::MAX),
                })
                .collect(),
        }
    }
}
