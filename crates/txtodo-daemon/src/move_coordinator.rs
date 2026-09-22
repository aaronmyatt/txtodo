//! Cross-file `Move` (plan §3.2.8, root todo.txt task 16): coordinates the source and destination
//! actors and relocates the task's `ref:` directory alongside it, with rollback on failure.
//! `server.rs`'s `apply` RPC routes here whenever a batch is a lone cross-file `Move` —
//! `mutation.rs`'s `move_ops` only ever records the *source* half of it.
//!
//! No new op kind and no shared op row: the source records `OpKind::Move` (it just leaves), the
//! destination records its own `OpKind::Insert` (plan §3.2.8's "moves ... to sit beside the
//! destination file" is a normal append there) — both replay, undo and checkout correctly on
//! their own document with no change to either. See tasks/daemon-ref-move/notes.md for the
//! alternative (a shared op keyed by `file` *or* `to_file`) and why this crate did not take it.

use crate::handle::{ActorError, ActorHandle, Applied};
use crate::mutation::{self, Mutation, TaskRef};
use crate::refdir::{move_ref_dir, ref_tag_edit};
use crate::state::{self, StateError};
use std::path::{Path, PathBuf};
use txtodo_core::{LineEnding, OwnedLine};
use txtodo_model::{FilePath, Principal, TaskId, WorkspaceLayout};

/// Moves `task` from `source`'s document to `dest`'s, appending it at the destination's end, and
/// relocates its `ref:` directory (if any) to sit beside the destination file, applying the
/// collision rule there (plan §3.2 rule 4). If anything fails after the source has already
/// recorded the departure, the whole operation is rolled back — the two documents end up exactly
/// as they started (rule 8) — and the failure is returned to the caller.
/// Who and what made the move: the principal, and the client (`ApplyRequest.source`, task
/// op-source) — carried through every commit the move makes, source and destination alike, so
/// `txtodo log` and the activity pane never show a hole for a move (task op-source-gaps).
#[derive(Clone, Debug)]
pub struct Origin {
    /// The user or agent.
    pub principal: Principal,
    /// The client (`cli`, `mcp`, `tui`, `desktop`), when the request named one.
    pub source: Option<String>,
}

impl Origin {
    /// One commit's arguments.
    async fn apply(
        &self,
        h: &ActorHandle,
        mutations: Vec<Mutation>,
    ) -> Result<Applied, ActorError> {
        h.apply_from(mutations, self.principal.clone(), self.source.clone())
            .await
    }
}

pub async fn move_task_across_files(
    source: &ActorHandle,
    dest: &ActorHandle,
    task: TaskRef,
    origin: Origin,
    (root, layout): (&Path, &WorkspaceLayout),
) -> Result<Applied, ActorError> {
    let contents = source.get().await?;
    let peeked = mutation::peek_line(&contents.bytes, &task)?;
    let to = dest.path().clone();
    let moved = origin
        .apply(source, vec![Mutation::Move { task, to }])
        .await?;
    if let Err(e) = origin
        .apply(
            dest,
            vec![Mutation::Add {
                line: peeked.line.clone(),
            }],
        )
        .await
    {
        let _ = reinsert_at_source(source, &peeked.line, &origin).await;
        return Err(e);
    }
    if let Some(slug) = &peeked.ref_slug
        && let Err(e) = relocate_ref_dir(
            source.path(),
            dest,
            (peeked.id, slug),
            (root, layout),
            &origin,
        )
        .await
    {
        let _ = remove_by_task_id(dest, peeked.id, &origin).await;
        let _ = reinsert_at_source(source, &peeked.line, &origin).await;
        return Err(e);
    }
    Ok(moved)
}

/// Rollback: puts the line back at the source, appended (its exact original position is not
/// preserved — a rare failure path, and losing the line would be worse than its old spot).
async fn reinsert_at_source(
    source: &ActorHandle,
    line: &str,
    origin: &Origin,
) -> Result<Applied, ActorError> {
    origin
        .apply(
            source,
            vec![Mutation::Add {
                line: line.to_owned(),
            }],
        )
        .await
}

/// The absolute directory `file` sits in.
fn dir_of(root: &Path, layout: &WorkspaceLayout, file: &FilePath) -> PathBuf {
    root.join(layout.refs_parent_of(file))
}

/// Moves the task's `ref:` directory (if it exists on disk — a dangling ref, rule 9, has nothing
/// to move) beside the destination file, then rewrites the destination's `ref:` tag if the
/// collision rule changed the slug.
async fn relocate_ref_dir(
    source_path: &FilePath,
    dest: &ActorHandle,
    (id, slug): (TaskId, &str),
    (root, layout): (&Path, &WorkspaceLayout),
    origin: &Origin,
) -> Result<(), ActorError> {
    let src_dir = dir_of(root, layout, source_path).join(slug);
    if !src_dir.exists() {
        return Ok(());
    }
    let dest_parent = dir_of(root, layout, dest.path());
    let final_slug = move_ref_dir(&src_dir, &dest_parent, slug)?;
    if final_slug != slug {
        rewrite_ref_tag(dest, id, &final_slug, origin).await?;
    }
    Ok(())
}

/// Rewrites the destination's `ref:` tag to `final_slug` — a plain text edit, no directory
/// operation: `move_ref_dir` has already physically relocated the directory by this point, so
/// this must not reuse `ActorHandle::rename_ref_dir` (which would try to move a directory of its
/// own and could clobber an unrelated one that happens to sit at the *old* slug name).
async fn rewrite_ref_tag(
    dest: &ActorHandle,
    id: TaskId,
    final_slug: &str,
    origin: &Origin,
) -> Result<(), ActorError> {
    let line_number = line_number_of(dest, id).await?;
    let task = TaskRef {
        line_number,
        task_id: Some(id),
    };
    let contents = dest.get().await?;
    let peeked = mutation::peek_line(&contents.bytes, &task)?;
    let owned = OwnedLine::from_bytes(peeked.line.into_bytes(), LineEnding::default());
    let new_line = txtodo_core::apply(&owned, &ref_tag_edit(Some(final_slug)));
    let new_line = new_line.raw().unwrap_or_default().to_owned();
    origin
        .apply(dest, vec![Mutation::Edit { task, new_line }])
        .await?;
    Ok(())
}

/// The rollback half of a failed relocation: removes the line this call already inserted at the
/// destination (a hard delete, like `Mutation::Delete`'s own `SetField { Deleted: true }`).
async fn remove_by_task_id(
    h: &ActorHandle,
    id: TaskId,
    origin: &Origin,
) -> Result<Applied, ActorError> {
    let line_number = line_number_of(h, id).await?;
    origin
        .apply(
            h,
            vec![Mutation::Delete {
                task: TaskRef {
                    line_number,
                    task_id: Some(id),
                },
                leave_blank: false,
            }],
        )
        .await
}

/// The 1-based line a task currently sits at, read fresh (a `Mutation::Add`'s append position is
/// not known to the caller ahead of time).
async fn line_number_of(h: &ActorHandle, id: TaskId) -> Result<usize, ActorError> {
    let contents = h.get().await?;
    let file = txtodo_core::parse_file(&contents.bytes);
    file.lines
        .iter()
        .position(|l| state::id_of(l) == Some(id))
        .map(|i| i + 1)
        .ok_or(ActorError::State(StateError::UnknownTask(id)))
}
