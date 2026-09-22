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

/// A line's `ref:` directory, resolved or (with `ensure`) created (`RefDir`; task
/// desktop-sublist-start). `dir` is workspace-relative, so `<dir>/todo.txt` is the sub-list's
/// path for `apply`.
#[derive(Debug, Clone, Serialize)]
pub struct RefDirInfoDto {
    /// The resolved task id (ULID text).
    pub task_id: String,
    /// The existing tag's slug, or the one creation used.
    pub slug: String,
    /// Workspace-relative directory for that slug.
    pub dir: String,
    /// Whether the line already carried a `ref:` tag before this call.
    pub has_ref_tag: bool,
    /// Whether `dir` exists on disk (after this call, if `ensure`).
    pub dir_exists: bool,
}

impl From<pb::RefDirInfo> for RefDirInfoDto {
    fn from(r: pb::RefDirInfo) -> RefDirInfoDto {
        RefDirInfoDto {
            task_id: r.task_id,
            slug: r.slug,
            dir: r.dir,
            has_ref_tag: r.has_ref_tag,
            dir_exists: r.dir_exists,
        }
    }
}
