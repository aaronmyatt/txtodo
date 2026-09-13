//! The cached workspace tree (plan M5, tasks/model-workspace-tree + tasks/proto-tree-progress):
//! `mark_dirty_for` is the write side, called by `FileActor::commit` for every batch; `workspace_tree`
//! is the read side, called by `server.rs`'s `ListFiles`/`Watch` and `refdir_grpc.rs`'s
//! `PruneOrphans`. The pure graph lives in `txtodo_model::WorkspaceTree` — this module is only the
//! seam that feeds it live counters and `ref:` tags without ever re-walking the filesystem or
//! recomputing a node an op could not have touched (`txtodo_model::invalidates`).
//!
//! ## Why a dirty flag and a full rebuild, not per-node patching
//! `invalidates` is precise enough to patch exactly the nodes one op can affect, but doing that
//! safely means keeping a parent's `children` map and a child's `owner` in sync from two different
//! call sites (the parent's own commit, and whichever future commit changes the child's directory
//! identity on a `Move`). A single dirty flag plus a full rebuild from live actor state is coarser
//! but cannot get that cross-node bookkeeping wrong, and "live actor state" here means in-memory
//! `ActorHandle` calls, not a disk walk — the one exception is folding in `notes.md`-only
//! directories, which hold no `FileActor` at all (see `rebuild_workspace_tree`). An op `invalidates`
//! says cannot touch the tree (a `NotesEdit`, a bare priority change, a blank) never even sets the
//! flag, which is the actual saving over "recompute on every op": most day-to-day edits are exactly
//! these.

use crate::convert::{file_kind_of, status_of};
use crate::handle::ActorHandle;
use crate::server::TxtodoService;
use crate::walker;
use std::path::Path;
use tonic::Status;
use txtodo_model::{NodeId, Op, Progress, RefTag, WorkspaceTree, invalidates};
use txtodo_proto::v1 as pb;

use crate::tree_dirty::TreeDirty;

/// Marks `dirty` when any of `ops` could change the cached workspace tree; a no-op otherwise
/// (`invalidates` decides per op, exhaustively over `OpKind`).
pub(crate) fn mark_dirty_for(dirty: &TreeDirty, ops: &[Op]) {
    let touches_tree = ops
        .iter()
        .any(|op| !invalidates(&op.kind, &op.file).is_empty());
    if touches_tree {
        dirty.mark();
    }
}

/// `tree`'s root as a `pb::TreeNode`, `files` grouped onto whichever node owns their directory
/// (`ListFiles`, plan M5). Recurses one level per tree depth, asserted against
/// `txtodo_model::MAX_TREE_DEPTH` (`__CLAUDE.md` §3: "no recursion unless depth is asserted
/// against a cap") — `WorkspaceTree::build` already refuses anything deeper.
pub(crate) fn to_pb_tree(tree: &WorkspaceTree, files: &[pb::FileInfo]) -> pb::TreeNode {
    build_node(tree, &NodeId::root(), files, 0)
}

fn build_node(
    tree: &WorkspaceTree,
    id: &NodeId,
    files: &[pb::FileInfo],
    depth: usize,
) -> pb::TreeNode {
    debug_assert!(
        depth <= txtodo_model::MAX_TREE_DEPTH,
        "tree depth is bounded"
    );
    let dir = id.as_dir().map(ToString::to_string).unwrap_or_default();
    let own_files = files
        .iter()
        .filter(|f| NodeId::of_file(&file_path_of(f)) == *id)
        .cloned()
        .collect();
    let children = tree
        .children(id)
        .map(|(_, child)| build_node(tree, child, files, depth + 1))
        .collect();
    pb::TreeNode {
        dir,
        progress: Some(pb_progress(tree.progress(id).unwrap_or_default())),
        owner_task_id: tree.owner(id).map(|t| t.to_string()).unwrap_or_default(),
        files: own_files,
        children,
    }
}

fn file_path_of(f: &pb::FileInfo) -> txtodo_model::FilePath {
    txtodo_model::FilePath::new(&f.path).unwrap_or_else(|_| {
        // `f.path` came from an `ActorHandle::path()` this same call already validated once at
        // the gRPC boundary; re-parsing it here can never fail in practice.
        txtodo_model::FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"))
    })
}

fn pb_progress(p: Progress) -> pb::Progress {
    pb::Progress {
        done: p.done,
        total: p.total,
    }
}

impl TxtodoService {
    /// The current workspace tree, rebuilding from live actor state first if anything since the
    /// last rebuild could have changed it.
    pub(crate) async fn workspace_tree(&self) -> Result<WorkspaceTree, Status> {
        if !self.workspace().tree_dirty.is_dirty() {
            return Ok(self
                .workspace()
                .cached_tree
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone());
        }
        let tree = self.rebuild_workspace_tree().await?;
        let ws = self.workspace();
        *ws.cached_tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = tree.clone();
        ws.tree_dirty.clear();
        Ok(tree)
    }

    /// Every directory holding a `todo.txt`/`done.txt` (from the registered actors) or a bare
    /// `notes.md` (from one bounded walk — the only document kind with no `FileActor` of its own,
    /// see `workspace.rs`'s module doc), each with its own rule-5 progress and `ref:` tags.
    async fn rebuild_workspace_tree(&self) -> Result<WorkspaceTree, Status> {
        let handles = self.all_actors();
        let mut inputs = Vec::with_capacity(handles.len() + 1);
        for h in &handles {
            if file_kind_of(h.path()) != pb::FileKind::Todo {
                continue; // done.txt folds into its sibling todo.txt's node below
            }
            inputs.push(self.node_input_for(h).await?);
        }
        for dir in notes_only_dirs(self.workspace().root(), &handles) {
            inputs.push(txtodo_model::NodeInput {
                id: dir,
                progress: Progress::default(),
                ref_tags: Vec::new(),
            });
        }
        WorkspaceTree::build(inputs).map_err(|e| Status::internal(format!("{e:?}")))
    }

    /// One `todo.txt`'s node: its own counters folded with its sibling `done.txt`'s (rule 5), and
    /// the union of both documents' `ref:` tags (rule 7: an archived line keeps its tag).
    async fn node_input_for(&self, todo: &ActorHandle) -> Result<txtodo_model::NodeInput, Status> {
        let pb_progress = self.progress_for(todo).await?;
        let mut ref_tags: Vec<RefTag> = todo.ref_tags().await.map_err(status_of)?;
        let sibling_path = crate::convert::sibling_done_path(todo.path());
        let sibling = self.workspace().actor(&sibling_path).cloned();
        if let Some(done) = sibling {
            ref_tags.extend(done.ref_tags().await.map_err(status_of)?);
        }
        Ok(txtodo_model::NodeInput {
            id: NodeId::of_file(todo.path()),
            progress: Progress {
                done: pb_progress.done,
                total: pb_progress.total,
            },
            ref_tags,
        })
    }
}

/// Directories that hold a `notes.md` but no registered `todo.txt`/`done.txt` actor of their own —
/// otherwise invisible to `rebuild_workspace_tree`, which only sees registered actors. A bounded
/// walk from `root`, run only while the tree is dirty (never per op).
fn notes_only_dirs(root: &Path, handles: &[ActorHandle]) -> Vec<NodeId> {
    let known: Vec<NodeId> = handles.iter().map(|h| NodeId::of_file(h.path())).collect();
    let Ok(found) = walker::walk(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<NodeId> = found
        .iter()
        .filter(|p| walker::is_notes_document(p.as_str().rsplit('/').next().unwrap_or(p.as_str())))
        .map(NodeId::of_file)
        .filter(|id| !known.contains(id))
        .collect();
    dirs.sort_by_key(|id| id.as_dir().map(|d| d.as_str().to_owned()));
    dirs.dedup();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use crate::mutation::Mutation;
    use crate::server::SharedWorkspace;
    use crate::workspace::Workspace;
    use std::sync::{Arc, RwLock};
    use txtodo_model::{IdentityMode, Principal};

    fn user(ws: &Workspace) -> Principal {
        Principal::User {
            device: ws.device(),
        }
    }

    fn service(root: &Path) -> TxtodoService {
        let ws =
            Workspace::open_with_default_mode(root, Arc::new(SystemClock), IdentityMode::Tagged)
                .unwrap_or_else(|e| panic!("{e}"));
        let ws: SharedWorkspace = Arc::new(RwLock::new(ws));
        TxtodoService::new(ws)
    }

    /// A parent → child ref: directory, on disk before the workspace ever opens, so the initial
    /// walk discovers both without any lazy-creation RPC involved (that machinery is
    /// `daemon-ref-creation`'s own, exercised in `refdir_tests.rs`).
    fn write_fixture(root: &Path) {
        std::fs::write(root.join("todo.txt"), "(A) Q4 roadmap ref:q4-roadmap\n")
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(root.join("q4-roadmap")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            root.join("q4-roadmap/todo.txt"),
            "buy ducks\nx 2020-01-01 completed thing\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(root.join("q4-roadmap/done.txt"), "archived thing\n")
            .unwrap_or_else(|e| panic!("{e}"));
    }

    #[tokio::test]
    async fn builds_the_tree_from_live_actors_with_rule_5_progress_and_owner() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        write_fixture(dir.path());
        let svc = service(dir.path());
        assert!(svc.workspace().tree_dirty.is_dirty(), "nothing cached yet");

        let tree = svc.workspace_tree().await.unwrap_or_else(|e| panic!("{e}"));
        assert!(
            !svc.workspace().tree_dirty.is_dirty(),
            "cleared after a build"
        );

        assert_eq!(
            tree.progress(&NodeId::root()),
            Some(Progress { done: 0, total: 1 })
        );
        let child = NodeId::dir(txtodo_model::FilePath::new("q4-roadmap").unwrap());
        assert_eq!(
            tree.progress(&child),
            Some(Progress { done: 2, total: 3 }),
            "1 completed + 1 archived done, 2 own + 1 archived total (rule 5)"
        );
        let kids: Vec<&str> = tree.children(&NodeId::root()).map(|(s, _)| s).collect();
        assert_eq!(kids, vec!["q4-roadmap"]);
        assert!(tree.owner(&child).is_some(), "the roadmap line owns it");
        assert!(tree.orphans().next().is_none());
    }

    #[tokio::test]
    async fn a_content_change_marks_the_cache_dirty_and_the_next_read_reflects_it() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        write_fixture(dir.path());
        let svc = service(dir.path());
        let _ = svc.workspace_tree().await.unwrap_or_else(|e| panic!("{e}"));
        assert!(!svc.workspace().tree_dirty.is_dirty());

        let handle = svc
            .actor_by_path(&txtodo_model::FilePath::new("q4-roadmap/todo.txt").unwrap())
            .unwrap_or_else(|e| panic!("{e}"));
        let principal = user(&svc.workspace());
        handle
            .apply(
                vec![Mutation::Add {
                    line: "one more".into(),
                }],
                principal,
            )
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            svc.workspace().tree_dirty.is_dirty(),
            "an Insert invalidates"
        );

        let child = NodeId::dir(txtodo_model::FilePath::new("q4-roadmap").unwrap());
        let tree = svc.workspace_tree().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            tree.progress(&child),
            Some(Progress { done: 2, total: 4 }),
            "the new line counts toward total, not done"
        );
    }
}
