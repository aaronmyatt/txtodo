//! `NotesDoc` DTO (plan M5, design §7): mirrors `FileContentsDto`'s shape for the same reason —
//! `notes.md` bytes decoded as UTF-8, lossily if not (markdown is always UTF-8 in practice, so the
//! bridge never panics on unexpected bytes). Split out of `dto.rs`; see that file's module doc.

use crate::dto::hex;
use serde::Serialize;
use txtodo_proto::v1 as pb;

/// `notes.md` for one task's `ref:` directory (`GetNotes`).
#[derive(Debug, Clone, Serialize)]
pub struct NotesDocDto {
    /// Workspace-relative `<ref>/notes.md`; empty until the first write.
    pub path: String,
    /// Current markdown text; empty when the file does not exist yet.
    pub text: String,
    /// Hex blake3 of the current bytes.
    pub hash: String,
}

impl From<pb::NotesDoc> for NotesDocDto {
    fn from(n: pb::NotesDoc) -> NotesDocDto {
        NotesDocDto {
            path: n.path,
            text: String::from_utf8_lossy(&n.bytes).into_owned(),
            hash: hex(&n.hash),
        }
    }
}
