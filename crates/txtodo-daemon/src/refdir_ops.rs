//! The `FileActor` operations built on `refdir.rs`'s pure slug/filesystem logic: lazy `ref:`
//! creation and rename-by-tag-edit (plan §3.2 rule 4, root todo.txt task 14). Split out for the
//! file budget; same ordering and rollback discipline as `refdir.rs`'s module doc.

use crate::actor::{Commit, CommitTail, FileActor};
use crate::handle::{ActorError, ActorMsg};
use crate::mutation::{self, TaskRef};
use crate::reconcile::change_ops;
use crate::refdir::{
    Claim, MAX_SLUG_COLLISIONS, RefDirError, RefDirInfo, fallback_slug, generate_slug,
    ref_tag_edit, slug_taken_in_doc, suffixed, task_view, try_create_dir,
};
use crate::state::StateError;
use std::fs;
use std::path::{Path, PathBuf};
use txtodo_core::OwnedLine;
use txtodo_model::{Principal, TaskId};

impl FileActor {
    /// Handles `EnsureRefDir`/`RenameRefDir` and hands everything else back unchanged — split out
    /// of `actor.rs::handle` so that function stays under its file's line budget as messages grow.
    pub(crate) fn handle_refdir(&mut self, msg: ActorMsg) -> Option<ActorMsg> {
        match msg {
            ActorMsg::EnsureRefDir {
                task,
                principal,
                reply,
            } => {
                let _ = reply.send(self.ensure_ref_dir(task, principal));
                None
            }
            ActorMsg::RenameRefDir {
                task,
                new_slug,
                principal,
                reply,
            } => {
                let _ = reply.send(self.rename_ref_dir(task, new_slug, principal));
                None
            }
            other => Some(other),
        }
    }

    /// Commits a description-only change to one task's line as its own one-tick batch (a no-op
    /// when the edit changes nothing). Shared by every `ref:` tag write in this module.
    fn commit_description_edit(
        &mut self,
        id: TaskId,
        new_line: OwnedLine,
        principal: &Principal,
    ) -> Result<(), ActorError> {
        let old = self.state.line_of(id).ok_or(StateError::UnknownTask(id))?;
        let kinds = change_ops(&old, &new_line, id);
        if kinds.is_empty() {
            return Ok(());
        }
        let ops = self.stamp(kinds, principal)?;
        let mut next = self.state.clone();
        for op in &ops {
            next.apply(op)?;
        }
        let bytes = next.to_bytes();
        let write = bytes != self.projection;
        self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail::default(),
        })?;
        Ok(())
    }

    /// The directory this file's own path sits in.
    fn own_dir(&self) -> PathBuf {
        self.cfg
            .disk
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.cfg.disk.clone())
    }

    /// Lazy `ref:` creation (plan §3.2 rule 4, root todo.txt task 14): if `task` already has a
    /// valid `ref:` tag, ensures its directory exists (idempotent, no op — a dangling ref, rule 9,
    /// just gets its directory back) and returns it. Otherwise mints a slug from the description,
    /// then commits the tag and claims the directory as one op batch: the tag lands first, the
    /// directory is claimed second, and a `-2`/`-3` collision (filesystem *or* another line's
    /// dangling tag) rolls the tag back and retries the next suffix.
    pub(crate) fn ensure_ref_dir(
        &mut self,
        task: TaskRef,
        principal: Principal,
    ) -> Result<RefDirInfo, ActorError> {
        let (_, id) = mutation::resolve(&self.state, &task)?;
        let line = self.state.line_of(id).ok_or(StateError::UnknownTask(id))?;
        let existing = task_view(&line).and_then(|t| t.ref_slug().map(str::to_owned));
        if let Some(slug) = existing {
            let dir = self.own_dir().join(&slug);
            fs::create_dir_all(&dir).map_err(|source| RefDirError::Io {
                op: "create directory",
                path: dir.clone(),
                source,
            })?;
            return Ok(RefDirInfo { slug, dir });
        }
        let base = task_view(&line)
            .map(|t| generate_slug(t.plain_words(), id))
            .unwrap_or_else(|| fallback_slug(id));
        self.claim_new_ref_dir(id, &line, &base, &principal)
    }

    /// The collision loop `ensure_ref_dir` runs once a slug base is known.
    fn claim_new_ref_dir(
        &mut self,
        id: TaskId,
        line: &OwnedLine,
        base: &str,
        principal: &Principal,
    ) -> Result<RefDirInfo, ActorError> {
        let dir_parent = self.own_dir();
        for attempt in 0..=MAX_SLUG_COLLISIONS {
            let candidate = suffixed(base, attempt);
            if slug_taken_in_doc(&self.state, &candidate, id) {
                continue;
            }
            let new_line = txtodo_core::apply(line, &ref_tag_edit(Some(&candidate)));
            self.commit_description_edit(id, new_line, principal)?;
            let dir = dir_parent.join(&candidate);
            match try_create_dir(&dir) {
                Ok(Claim::Created) => {
                    return Ok(RefDirInfo {
                        slug: candidate,
                        dir,
                    });
                }
                Ok(Claim::Taken) => {
                    self.rollback_ref_tag(id, principal)?;
                }
                Err(source) => {
                    self.rollback_ref_tag(id, principal)?;
                    return Err(RefDirError::Io {
                        op: "create directory",
                        path: dir,
                        source,
                    }
                    .into());
                }
            }
        }
        Err(RefDirError::CollisionsExhausted.into())
    }

    /// Removes the `ref:` tag added by a failed `ensure_ref_dir`/`rename_ref_dir` attempt: the
    /// rollback half of the ordering `refdir.rs`'s module doc describes.
    fn rollback_ref_tag(&mut self, id: TaskId, principal: &Principal) -> Result<(), ActorError> {
        let line = self.state.line_of(id).ok_or(StateError::UnknownTask(id))?;
        let new_line = txtodo_core::apply(&line, &ref_tag_edit(None));
        self.commit_description_edit(id, new_line, principal)
    }

    /// Renames an existing `ref:` slug (the user edited it directly, plan §3.2 rule 4): the tag
    /// is rewritten first, then the directory — if one exists, a dangling ref (rule 9) has
    /// nothing to move — is renamed to match, rolling the tag back on failure. Rejects a target
    /// slug already claimed in this document rather than silently substituting another name.
    pub(crate) fn rename_ref_dir(
        &mut self,
        task: TaskRef,
        new_slug: String,
        principal: Principal,
    ) -> Result<RefDirInfo, ActorError> {
        if !txtodo_core::is_valid_slug(&new_slug) {
            return Err(RefDirError::InvalidSlug(new_slug).into());
        }
        let (_, id) = mutation::resolve(&self.state, &task)?;
        let line = self.state.line_of(id).ok_or(StateError::UnknownTask(id))?;
        let old_slug = task_view(&line).and_then(|t| t.ref_slug().map(str::to_owned));
        if old_slug.as_deref() == Some(new_slug.as_str()) {
            let dir = self.own_dir().join(&new_slug);
            return Ok(RefDirInfo {
                slug: new_slug,
                dir,
            });
        }
        if slug_taken_in_doc(&self.state, &new_slug, id) {
            return Err(RefDirError::SlugTaken(new_slug).into());
        }
        let new_line = txtodo_core::apply(&line, &ref_tag_edit(Some(&new_slug)));
        self.commit_description_edit(id, new_line, &principal)?;
        self.finish_rename(id, old_slug, new_slug, &principal)
    }

    /// The filesystem half of `rename_ref_dir`, after the tag has already landed.
    fn finish_rename(
        &mut self,
        id: TaskId,
        old_slug: Option<String>,
        new_slug: String,
        principal: &Principal,
    ) -> Result<RefDirInfo, ActorError> {
        let dir_parent = self.own_dir();
        let new_dir = dir_parent.join(&new_slug);
        let Some(old_slug) = old_slug else {
            return Ok(RefDirInfo {
                slug: new_slug,
                dir: new_dir,
            });
        };
        let old_dir = dir_parent.join(&old_slug);
        if !old_dir.exists() {
            return Ok(RefDirInfo {
                slug: new_slug,
                dir: new_dir,
            });
        }
        match fs::rename(&old_dir, &new_dir) {
            Ok(()) => Ok(RefDirInfo {
                slug: new_slug,
                dir: new_dir,
            }),
            Err(source) => {
                self.rollback_rename_tag(id, &old_slug, principal)?;
                Err(RefDirError::Io {
                    op: "rename directory",
                    path: old_dir,
                    source,
                }
                .into())
            }
        }
    }

    fn rollback_rename_tag(
        &mut self,
        id: TaskId,
        old_slug: &str,
        principal: &Principal,
    ) -> Result<(), ActorError> {
        let line = self.state.line_of(id).ok_or(StateError::UnknownTask(id))?;
        let new_line = txtodo_core::apply(&line, &ref_tag_edit(Some(old_slug)));
        self.commit_description_edit(id, new_line, principal)
    }
}
