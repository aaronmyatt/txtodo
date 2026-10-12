//! Serde-friendly mirrors of the generated protobuf messages that cross the Tauri IPC bridge.
//! Prost messages don't derive `Serialize`/`Deserialize` (see <https://docs.rs/prost>), so every
//! command converts to/from these instead of leaking `txtodo_proto` types to the frontend. Hash
//! fields cross as lowercase hex, never raw bytes.
//! Ref: <https://v2.tauri.app/develop/calling-rust/#returning-data>

use serde::{Deserialize, Serialize};
use txtodo_proto::v1 as pb;

// Split out of this file for the same reason `crates/txtodo-daemon/src/server.rs` delegates to
// `notes.rs`/`tokens.rs`/`pairing_grpc.rs`/`activity.rs`: keeping every DTO in one file would blow
// the line budget. Each sibling re-exports through here so callers keep a single `crate::dto::*`
// import surface, unaware of the split.
pub use crate::dto_activity::AggregatedOpLogEntryDto;
pub use crate::dto_notes::{NotesDocDto, RefDirInfoDto};
pub use crate::dto_pairing::{PairOfferDto, PairResultDto};
pub use crate::dto_tokens::TokenDto;
pub use crate::dto_workspace::{WorkspaceInfoDto, WorkspaceLayoutDto, is_ready_or_unknown};

/// Lowercase-hex encoding of a byte slice (blake3 projection hashes are 32 bytes). `pub(crate)`
/// (not private) so `dto_notes.rs` — a sibling module, not a descendant of this one — can reuse it
/// for `NotesDoc`'s hash instead of duplicating the encoding.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The inverse of [`hex`]. Anything that is not an even run of hex digits decodes to no bytes at
/// all: as a `Replace` base that matches no document, so the daemon refuses the write
/// (`FAILED_PRECONDITION`, nothing written) rather than this bridge guessing.
/// `u8::from_str_radix`: https://doc.rust-lang.org/std/primitive.u8.html#method.from_str_radix
pub(crate) fn unhex(text: &str) -> Vec<u8> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return Vec::new();
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .unwrap_or_default()
}

/// One synced document (`ListFiles`).
#[derive(Debug, Clone, Serialize)]
pub struct FileInfoDto {
    /// Workspace-relative path, `/` separators.
    pub path: String,
    /// Hex blake3 of the projection.
    pub hash: String,
    /// `"FILE_KIND_TODO"` / `"FILE_KIND_NOTES"` / `"FILE_KIND_UNSPECIFIED"`.
    pub kind: String,
    /// Completed task lines; 0 for notes.md.
    pub done: u32,
    /// Total task lines; 0 for notes.md.
    pub total: u32,
}

impl From<pb::FileInfo> for FileInfoDto {
    fn from(f: pb::FileInfo) -> FileInfoDto {
        let kind = pb::FileKind::try_from(f.kind).unwrap_or(pb::FileKind::Unspecified);
        FileInfoDto {
            path: f.path,
            hash: hex(&f.hash),
            kind: kind.as_str_name().to_owned(),
            done: f.progress.as_ref().map_or(0, |p| p.done),
            total: f.progress.as_ref().map_or(0, |p| p.total),
        }
    }
}

/// The exact bytes for one document (`GetFile`), decoded as UTF-8 (todo.txt files always are —
/// lossily if not, so the bridge never panics on unexpected bytes).
#[derive(Debug, Clone, Serialize)]
pub struct FileContentsDto {
    /// Workspace-relative path.
    pub path: String,
    /// The document's text.
    pub text: String,
    /// Hex blake3 of the projection.
    pub hash: String,
    /// One entry per line of `text`: the line's task id (ULID text), `""` for a blank line. Under
    /// Sidecar identity a line has no `id:` tag, so this is where the frontend gets the id for a
    /// `TaskRef` (task sidecar-task-ids). Empty from a daemon older than the field.
    pub task_ids: Vec<String>,
}

impl From<pb::FileContents> for FileContentsDto {
    fn from(f: pb::FileContents) -> FileContentsDto {
        FileContentsDto {
            path: f.path,
            text: String::from_utf8_lossy(&f.bytes).into_owned(),
            hash: hex(&f.hash),
            task_ids: f.task_ids,
        }
    }
}

/// One recorded op (`History`/`Change`).
#[derive(Debug, Clone, Serialize)]
pub struct OpSummaryDto {
    /// Monotonic sequence number.
    pub seq: i64,
    /// ULID text.
    pub op_id: String,
    /// ULID text.
    pub device: String,
    /// `"you@dev"` / `"agent:name@dev"` / `"external@dev"`.
    pub principal: String,
    /// Store kind tag: insert, set_field, edit_text, move, ...
    pub kind: String,
    /// ULID text; empty for blank-line ops.
    pub task_id: String,
    /// One-line human summary.
    pub summary: String,
}

impl From<pb::OpSummary> for OpSummaryDto {
    fn from(o: pb::OpSummary) -> OpSummaryDto {
        OpSummaryDto {
            seq: o.seq,
            op_id: o.op_id,
            device: o.device,
            principal: o.principal,
            kind: o.kind,
            task_id: o.task_id,
            summary: o.summary,
        }
    }
}

/// One `Watch` event, forwarded to the frontend as a `daemon-change` Tauri event.
#[derive(Debug, Clone, Serialize)]
pub struct ChangeDto {
    /// Workspace-relative path that changed.
    pub path: String,
    /// Hex blake3 of the projection after the change.
    pub hash: String,
    /// Ops this change appended.
    pub ops: Vec<OpSummaryDto>,
    /// The file's duplicate groups after the change (ADR 0032); 0 from an older daemon.
    pub duplicate_groups: u32,
}

impl From<pb::Change> for ChangeDto {
    fn from(c: pb::Change) -> ChangeDto {
        ChangeDto {
            path: c.path,
            hash: hex(&c.hash),
            ops: c.ops.into_iter().map(OpSummaryDto::from).collect(),
            duplicate_groups: c.duplicate_groups,
        }
    }
}

/// Result of `Apply`.
#[derive(Debug, Clone, Serialize)]
pub struct ApplyResultDto {
    /// Ops appended.
    pub applied: u32,
    /// Hex blake3 of the projection after the write.
    pub hash: String,
    /// HLC wall-clock component of the last op.
    pub hlc_wall_ms: u64,
    /// HLC counter component of the last op.
    pub hlc_counter: u32,
}

impl From<pb::ApplyResponse> for ApplyResultDto {
    fn from(a: pb::ApplyResponse) -> ApplyResultDto {
        ApplyResultDto {
            applied: a.applied,
            hash: hex(&a.hash),
            hlc_wall_ms: a.hlc_wall_ms,
            hlc_counter: a.hlc_counter,
        }
    }
}

/// `History` result.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryDto {
    /// Ops newest first.
    pub ops: Vec<OpSummaryDto>,
}

impl From<pb::HistoryResponse> for HistoryDto {
    fn from(h: pb::HistoryResponse) -> HistoryDto {
        HistoryDto {
            ops: h.ops.into_iter().map(OpSummaryDto::from).collect(),
        }
    }
}

/// A line addressed by the frontend: 1-based line number and/or a ULID task id (empty when the
/// line has none yet). Mirrors `pb::TaskRef`.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskRefDto {
    /// 1-based over every line, blanks included.
    pub line_number: u32,
    /// ULID text; empty when the line has no id yet.
    pub task_id: String,
}

impl From<TaskRefDto> for pb::TaskRef {
    fn from(t: TaskRefDto) -> pb::TaskRef {
        pb::TaskRef {
            line_number: t.line_number,
            task_id: t.task_id,
        }
    }
}

/// One intent-level mutation the frontend can ask `Apply` to make.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationDto {
    /// Appends a full line; the daemon assigns an id if the text has none.
    Add {
        /// Full line text, no line ending.
        line: String,
    },
    /// Marks a task line done as of `today`.
    Complete {
        /// The line to complete.
        task: TaskRefDto,
        /// `YYYY-MM-DD` in the client's local zone (ADR 0011).
        today: String,
    },
    /// Whole-line replacement; the daemon derives field-level ops.
    Edit {
        /// The line to replace.
        task: TaskRefDto,
        /// The replacement line text.
        new_line: String,
    },
    /// Moves a task line to another workspace-relative file.
    Move {
        /// The line to move.
        task: TaskRefDto,
        /// Destination workspace-relative path.
        to_path: String,
    },
    /// Deletes a task line.
    Delete {
        /// The line to delete.
        task: TaskRefDto,
        /// `todo.sh` leaves a blank line by default.
        leave_blank: bool,
    },
    /// Moves a task line right before another in the same document (`pb::MoveBefore`): the top
    /// "Add a line" input's second step (root todo line on the add input; TUI `list.add_on_top`).
    MoveBefore {
        /// The line to move.
        task: TaskRefDto,
        /// The line it lands in front of.
        before: TaskRefDto,
    },
    /// The whole document, compare-and-swap (`pb::Replace`, daemon `replace.rs`): refused unless
    /// the document's hash is still `base_hash`, else reconciled like an external edit, so an
    /// untouched line keeps its identity and a moved line is a move. The editor saves with it
    /// (task desktop-reorder-propagates): per-line mutations cannot say "this line moved". Must be
    /// the only mutation of its `apply` call.
    Replace {
        /// Hex blake3 the buffer was edited from (`FileContentsDto.hash`).
        base_hash: String,
        /// The new document text.
        contents: String,
    },
}

impl From<MutationDto> for pb::Mutation {
    fn from(m: MutationDto) -> pb::Mutation {
        let kind = match m {
            MutationDto::Add { line } => pb::mutation::Kind::Add(pb::Add { line }),
            MutationDto::Complete { task, today } => pb::mutation::Kind::Complete(pb::Complete {
                task: Some(task.into()),
                today,
            }),
            MutationDto::Edit { task, new_line } => pb::mutation::Kind::Edit(pb::Edit {
                task: Some(task.into()),
                new_line,
            }),
            MutationDto::Move { task, to_path } => pb::mutation::Kind::Move(pb::Move {
                task: Some(task.into()),
                to_path,
            }),
            MutationDto::Delete { task, leave_blank } => pb::mutation::Kind::Delete(pb::Delete {
                task: Some(task.into()),
                leave_blank,
            }),
            MutationDto::MoveBefore { task, before } => {
                pb::mutation::Kind::MoveBefore(pb::MoveBefore {
                    task: Some(task.into()),
                    before: Some(before.into()),
                })
            }
            MutationDto::Replace {
                base_hash,
                contents,
            } => pb::mutation::Kind::Replace(pb::Replace {
                base_hash: unhex(&base_hash),
                contents: contents.into_bytes(),
            }),
        };
        pb::Mutation { kind: Some(kind) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unhex_inverts_hex_and_refuses_anything_else() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        assert_eq!(unhex(&hex(&bytes)), bytes);
        assert_eq!(unhex("ABcd"), vec![0xab, 0xcd]);
        assert!(unhex("abc").is_empty(), "odd length");
        assert!(unhex("zz").is_empty(), "not hex");
        assert!(unhex("é0").is_empty(), "not ascii");
    }

    #[test]
    fn a_move_before_mutation_names_both_lines() {
        let json = r#"{"kind":"move_before","task":{"line_number":6,"task_id":""},"before":{"line_number":2,"task_id":"01M2T868JD32M84JQQ2ABASXW4"}}"#;
        let dto: MutationDto = serde_json::from_str(json).unwrap_or_else(|e| panic!("{e}"));
        let pb::Mutation { kind } = dto.into();
        let Some(pb::mutation::Kind::MoveBefore(m)) = kind else {
            panic!("expected MoveBefore");
        };
        assert_eq!(m.task.map(|t| t.line_number), Some(6));
        assert_eq!(m.before.map(|t| t.line_number), Some(2));
    }

    #[test]
    fn a_replace_mutation_carries_the_decoded_base_and_the_text_bytes() {
        // The frontend sends `{ kind: "replace", ... }` (serde tag, same as every other arm).
        let json = r#"{"kind":"replace","base_hash":"0aff","contents":"b\na\n"}"#;
        let dto: MutationDto = serde_json::from_str(json).unwrap_or_else(|e| panic!("{e}"));
        let pb::Mutation { kind } = dto.into();
        let Some(pb::mutation::Kind::Replace(r)) = kind else {
            panic!("expected Replace");
        };
        assert_eq!(r.base_hash, vec![0x0a, 0xff]);
        assert_eq!(r.contents, b"b\na\n");
    }
}
