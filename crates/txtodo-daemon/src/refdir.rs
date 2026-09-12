//! `ref:` directory slugs (plan §3.2 rules 1, 4): kebab-case generation and collision-safe
//! filesystem moves. `refdir_ops.rs` holds the `FileActor` operations that create or rename a
//! task's directory using these; `move_coordinator.rs` reuses [`move_ref_dir`] from here for
//! cross-file relocation — the collision rule is one piece of logic, not two
//! (tasks/daemon-ref-move/notes.md).
//!
//! Every caller of this module follows the same order: commit the tag change first, then touch
//! the filesystem. A crash or failure between the two leaves a *dangling* ref (rule 9: tag
//! present, directory missing) rather than an *orphan* directory (rule 10: needs
//! `prune --orphans`) — dangling self-heals on the next lazy write, an orphan does not. On a
//! filesystem failure the caller rolls the tag back by committing the inverse edit, so the net
//! effect is as if the call never happened (see tasks/daemon-ref-creation/notes.md).

use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::mutation::TaskRef;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use txtodo_core::{Edit, LineKind};
use txtodo_model::{Principal, TaskId};

impl ActorHandle {
    /// Lazy `ref:` creation: the tag and directory in one op batch. Moved out of `handle.rs`
    /// purely to keep that file within its line budget — `ActorHandle::ask`/`send` are
    /// `pub(crate)` for exactly this sibling-module use.
    pub async fn ensure_ref_dir(
        &self,
        task: TaskRef,
        principal: Principal,
    ) -> Result<RefDirInfo, ActorError> {
        self.ask(|reply| ActorMsg::EnsureRefDir {
            task,
            principal,
            reply,
        })
        .await?
    }

    /// Renames an existing `ref:` slug and its directory to match.
    pub async fn rename_ref_dir(
        &self,
        task: TaskRef,
        new_slug: String,
        principal: Principal,
    ) -> Result<RefDirInfo, ActorError> {
        self.ask(|reply| ActorMsg::RenameRefDir {
            task,
            new_slug,
            principal,
            reply,
        })
        .await?
    }
}

/// Generate-side slug length (plan §3.2 rule 4: "truncated to 40 chars"). Deliberately narrower
/// than `txtodo_core::SLUG_MAX_LEN` (64, the *accept* limit for a hand-written tag): generating
/// narrower leaves headroom for a `-2`/`-3` suffix without ever crossing the parse limit. See
/// tasks/daemon-ref-creation/notes.md "40 and 64 are both right — do not fix the mismatch".
pub const SLUG_GENERATE_MAX_LEN: usize = 40;
const _: () = assert!(SLUG_GENERATE_MAX_LEN < txtodo_core::SLUG_MAX_LEN);

/// Bounded retry for a slug collision loop (`-2`, `-3`, ...). This many same-slug tasks beside one
/// file is not a real workspace, so hitting it is a bug or an attack, not bad luck.
pub const MAX_SLUG_COLLISIONS: u32 = 1_000;

/// Why a ref-directory operation failed. Any committed tag change is rolled back before this
/// reaches the caller (see the module doc).
#[derive(Debug)]
pub enum RefDirError {
    /// Every `base`, `base-2`, ... up to `MAX_SLUG_COLLISIONS` was taken.
    CollisionsExhausted,
    /// A user-typed slug fails the `ref:` grammar (plan §3.2.1).
    InvalidSlug(String),
    /// An explicit rename's target slug is already claimed in this document.
    SlugTaken(String),
    /// A filesystem step failed.
    Io {
        /// What was attempted.
        op: &'static str,
        /// The path.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
}

impl fmt::Display for RefDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefDirError::CollisionsExhausted => {
                write!(f, "no free ref: slug after {MAX_SLUG_COLLISIONS} attempts")
            }
            RefDirError::InvalidSlug(s) => write!(f, "{s:?} is not a valid ref: slug"),
            RefDirError::SlugTaken(s) => write!(f, "ref: slug {s:?} is already in use"),
            RefDirError::Io { op, path, source } => {
                write!(f, "cannot {op} {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RefDirError {}

impl From<RefDirError> for ActorError {
    fn from(e: RefDirError) -> ActorError {
        ActorError::RefDir(e)
    }
}

/// The directory a `ref:` operation created, renamed or confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefDirInfo {
    /// The slug actually used (may differ from a requested one on collision).
    pub slug: String,
    /// Its absolute path on disk.
    pub dir: PathBuf,
}

/// Kebab-case slug from a task's plain words (`Task::plain_words`), generate-truncated. Falls
/// back to a lowercase ULID when no ASCII plain word survives (an all-CJK or all-emoji
/// description) — seeing an empty slug would otherwise be silently produced, which is the bug
/// tasks/daemon-ref-creation/notes.md calls out.
pub fn generate_slug<'a>(plain_words: impl Iterator<Item = &'a str>, id: TaskId) -> String {
    let mut base = String::new();
    for word in plain_words {
        let mut piece = String::new();
        for ch in word.chars() {
            if ch.is_ascii_alphanumeric() {
                piece.push(ch.to_ascii_lowercase());
            } else if !piece.is_empty() && !piece.ends_with('-') {
                piece.push('-');
            }
        }
        let piece = piece.trim_end_matches('-');
        if piece.is_empty() {
            continue;
        }
        if !base.is_empty() {
            base.push('-');
        }
        base.push_str(piece);
    }
    // A description starting with a digit is fine; one starting with punctuation is not
    // (`is_valid_slug` requires `[a-z0-9]` first) — strip rather than reject.
    let base = base.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    let truncated: String = base.chars().take(SLUG_GENERATE_MAX_LEN).collect();
    let truncated = truncated.trim_end_matches('-');
    if truncated.is_empty() {
        fallback_slug(id)
    } else {
        truncated.to_owned()
    }
}

/// A short, stable, always-valid slug derived from the task's own id (base32 ULIDs are already
/// `[0-9A-Z]`, so lowercasing is the whole transform).
pub(crate) fn fallback_slug(id: TaskId) -> String {
    id.ulid().to_string().to_lowercase()
}

/// The `attempt`-th collision candidate: `base`, then `base-2`, `base-3`, ... (plan §3.2 rule 4).
pub(crate) fn suffixed(base: &str, attempt: u32) -> String {
    if attempt == 0 {
        base.to_owned()
    } else {
        format!("{base}-{}", attempt + 1)
    }
}

/// True when `candidate` is already claimed by another task's `ref:` tag in this document — a
/// dangling ref (rule 9) still claims its slug even with no directory on disk.
pub(crate) fn slug_taken_in_doc(
    state: &crate::state::DocState,
    candidate: &str,
    exclude: TaskId,
) -> bool {
    (0..state.len()).any(|i| {
        let Some(entry) = state.entry_at(i) else {
            return false;
        };
        entry.id() != Some(exclude) && entry.id().is_some() && {
            let Some(parsed) = entry.line().parse() else {
                return false;
            };
            matches!(parsed.kind, LineKind::Task(t) if t.ref_slug() == Some(candidate))
        }
    })
}

/// One exclusive `mkdir`: `Created` when this call made the directory, `Taken` when it already
/// existed (try the next suffix) — never a lookup-then-create, so two racing lazy creations of
/// the same slug cannot both "win" (tasks/daemon-ref-creation/notes.md).
pub(crate) enum Claim {
    /// This call created the directory.
    Created,
    /// It already existed.
    Taken,
}

pub(crate) fn try_create_dir(dir: &Path) -> io::Result<Claim> {
    match fs::create_dir(dir) {
        Ok(()) => Ok(Claim::Created),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(Claim::Taken),
        Err(e) => Err(e),
    }
}

/// Moves `src` to sit beside `dest_parent`, trying `base_slug`, then `base_slug-2`, ... Returns
/// the slug actually used. A pre-existence check guards the POSIX quirk where `rename` onto an
/// *empty* directory silently replaces it instead of erroring — not the exclusive-create
/// semantics `try_create_dir` gets from `mkdir`, but adequate for a same-workspace move, which
/// `move_coordinator` already serialises through one document's actor at a time.
pub(crate) fn move_ref_dir(
    src: &Path,
    dest_parent: &Path,
    base_slug: &str,
) -> Result<String, RefDirError> {
    for attempt in 0..=MAX_SLUG_COLLISIONS {
        let candidate = suffixed(base_slug, attempt);
        let dest = dest_parent.join(&candidate);
        if dest.symlink_metadata().is_ok() {
            continue;
        }
        match fs::rename(src, &dest) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(RefDirError::Io {
                    op: "move directory",
                    path: dest,
                    source,
                });
            }
        }
    }
    Err(RefDirError::CollisionsExhausted)
}

/// An `Edit` that sets or clears the `ref:` tag. The key is a fixed, always-valid literal, so a
/// build failure (which cannot happen) degrades to a no-op rather than panicking.
pub(crate) fn ref_tag_edit(value: Option<&str>) -> Edit {
    match value {
        Some(v) => Edit::new().set_tag("ref", v).unwrap_or_default(),
        None => Edit::new().remove_tag("ref").unwrap_or_default(),
    }
}

/// The task view of a line, when it parses as one (every entry `ensure_ref_dir`/`rename_ref_dir`
/// reach through `mutation::resolve` is a task line by construction; `None` here would be a bug).
pub(crate) fn task_view(line: &txtodo_core::OwnedLine) -> Option<txtodo_core::Task<'_>> {
    match line.parse()?.kind {
        LineKind::Task(t) => Some(t),
        LineKind::Blank => None,
    }
}
