//! The workspace tree (plan M5, specs/ref-directories.md rule 2): the graph of `ref:` directories
//! plus the workspace root, each with its own rule-5 progress. Pure and cache-shaped by
//! construction — the daemon feeds it a snapshot of directories/counters/ref-tags and asks which
//! nodes an op can have invalidated ([`invalidates`]); this module never walks a filesystem or
//! parses a line itself (no I/O in this crate — see the crate doc).
//!
//! ## A genuine tree, not the general graph it could have been
//! specs/ref-directories.md rule 2: a `ref:` slug always names a directory *beside the file
//! holding the line*, so a node's parent is fixed by its own path (its dirname), never by which
//! tag happens to name it. That makes a cycle structurally impossible here — a child's path is
//! always strictly longer than its parent's — so [`WorkspaceTree::build`] never has to detect one;
//! [`MAX_TREE_DEPTH`] is still asserted as the documented second line of defence (a hand-made
//! symlink or a hand-edited tag is exactly the kind of thing that should hit a bound, not a stack
//! overflow). Two things rule 2 makes explicit and worth keeping separate:
//! - A directory nothing points at is a legitimate node with no owner
//!   ([`WorkspaceTree::orphans`], rule 10, `prune --orphans`).
//! - A `ref:` tag naming a directory that does not exist yet is a dangling edge, not an error
//!   (rule 9); [`WorkspaceTree::build`] simply produces no child for it.
//!
//! ## Rule 5 is not recursive
//! `done`/`total` on a node are its own immediate sub-list only — never the sum of its children's.
//! Rule 5 is normative on this point, so "surely it should sum the grandchildren" is a bug, not a
//! reading of the spec.

use crate::{Field, FilePath, OpKind, TaskId, WorkspaceLayout};
use std::collections::BTreeMap;

/// Deepest `ref:` nesting the tree will model. specs/ref-directories.md rule 2's nesting is
/// unbounded in principle; this is far above any sane workspace and matches
/// `txtodo-daemon::walker::WALK_MAX_DEPTH`, so a walked workspace can never itself exceed it.
pub const MAX_TREE_DEPTH: usize = 32;

/// Most directories one cached tree may hold. "One entry per `ref:` directory in the workspace" is
/// an unbounded collection unless bounded (see tasks/model-workspace-tree/notes.md); matches
/// `txtodo-daemon::walker::WALK_MAX_FILES`, the walker's own document cap.
pub const MAX_TRACKED_REFS: usize = 10_000;

/// Rule 5's counters for one node: `done` = completed lines in its own `todo.txt`; `total` = task
/// lines, blanks excluded. Never recursive (see the module doc).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Completed and archived task lines.
    pub done: u32,
    /// All task lines, blanks excluded.
    pub total: u32,
}

/// One node's identity: the workspace root, or a `ref:` directory named by its workspace-relative
/// path. Reuses [`FilePath`]'s validation (relative, `/`-separated, no `..`) since a directory
/// path has exactly the same shape as a document path — it is just never a document itself.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(Option<FilePath>);

impl NodeId {
    /// The workspace root: it owns no `ref:` tag and has no directory path of its own.
    pub fn root() -> NodeId {
        NodeId(None)
    }

    /// A `ref:` directory at `dir` (workspace-relative, e.g. `"buy-ducks"` or
    /// `"buy-ducks/sync-section"`).
    pub fn dir(dir: FilePath) -> NodeId {
        NodeId(Some(dir))
    }

    /// True for the workspace root.
    pub fn is_root(&self) -> bool {
        self.0.is_none()
    }

    /// The directory path, for anything but the root.
    pub fn as_dir(&self) -> Option<&FilePath> {
        self.0.as_ref()
    }

    /// Nesting depth: 0 for the root, 1 for a top-level `ref:` directory, and so on.
    pub fn depth(&self) -> usize {
        self.0
            .as_ref()
            .map_or(0, |d| d.as_str().matches('/').count() + 1)
    }

    /// The node that owns `file` (a `todo.txt`/`notes.md` path): the directory `file`
    /// sits in, or the root when `file` has no `/`.
    pub fn of_file(file: &FilePath) -> NodeId {
        match file.as_str().rsplit_once('/') {
            // `dir` is a non-empty prefix of an already-validated `FilePath` with no trailing
            // slash and no `..` segment, so it is always itself a valid `FilePath`.
            Some((dir, _name)) => FilePath::new(dir).map_or_else(|_| NodeId::root(), NodeId::dir),
            None => NodeId::root(),
        }
    }

    /// The child directory this node would have for `slug` (rule 2: beside the file holding the
    /// line, i.e. inside this node's own directory). `None` only when `slug` itself is not a valid
    /// path segment, which `Task::ref_slug`'s grammar check already rules out for a real tag.
    ///
    /// The node holding the workspace's root list takes its child directories from the workspace
    /// layout (`refs_dir`, task workspace-layout); every other node, a nested list, keeps them
    /// beside its own file.
    fn child(&self, slug: &str, layout: &WorkspaceLayout) -> Option<NodeId> {
        let joined = if *self == NodeId::of_file(&layout.root_list()) {
            layout.ref_dir_of(slug)
        } else {
            match &self.0 {
                Some(dir) => format!("{}/{slug}", dir.as_str()),
                None => slug.to_owned(),
            }
        };
        FilePath::new(&joined).ok().map(NodeId::dir)
    }
}

/// One `ref:` tag found in a node's own `todo.txt` (rule 7: an archived line keeps its
/// tag), as the caller — the daemon, which owns parsing and the filesystem — already found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefTag {
    /// The task carrying the tag.
    pub owner: TaskId,
    /// Its `ref:` value; the caller only ever reports a tag `Task::ref_slug` itself accepted, so
    /// this is always grammar-valid.
    pub slug: String,
}

/// Everything the tree needs about one discovered directory. The tree model does not discover
/// anything (see the module doc) — the caller (`txtodo-daemon`) walks the filesystem, computes
/// rule-5 counters and scans `ref:` tags, and reports the result here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeInput {
    /// This directory.
    pub id: NodeId,
    /// Its own rule-5 progress.
    pub progress: Progress,
    /// `ref:` tags found in its own `todo.txt`.
    pub ref_tags: Vec<RefTag>,
}

/// One resolved node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Node {
    progress: Progress,
    /// The task whose `ref:` tag points here; `None` for the root and for an orphan (rule 10).
    owner: Option<TaskId>,
    /// Child directories that exist, by slug. A slug this node's tags name but that resolves to no
    /// discovered directory is a dangling edge (rule 9) and is never in this map.
    children: BTreeMap<String, NodeId>,
}

/// Why building a tree was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// More than [`MAX_TRACKED_REFS`] nodes.
    TooMany(usize),
    /// A node nests deeper than [`MAX_TREE_DEPTH`].
    TooDeep(NodeId),
}

/// The workspace tree: the root plus every discovered `ref:` directory, with rule-5 progress and
/// child edges (see the module doc for why this is a genuine tree, not a general graph).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceTree {
    nodes: BTreeMap<NodeId, Node>,
}

impl WorkspaceTree {
    /// Builds a tree from every discovered directory. `inputs` need not include the root; one is
    /// synthesised (empty progress, no owner) when absent, so a caller never has to special-case
    /// it.
    pub fn build(inputs: Vec<NodeInput>) -> Result<WorkspaceTree, TreeError> {
        WorkspaceTree::build_with_layout(inputs, &WorkspaceLayout::beside_the_list())
    }

    /// `build`, with the root list's `ref:` directories placed by `layout` (task
    /// workspace-layout): under `layout.refs_dir()`, or beside the list when it is `.`.
    pub fn build_with_layout(
        inputs: Vec<NodeInput>,
        layout: &WorkspaceLayout,
    ) -> Result<WorkspaceTree, TreeError> {
        if inputs.len() > MAX_TRACKED_REFS {
            return Err(TreeError::TooMany(inputs.len()));
        }
        for input in &inputs {
            if input.id.depth() > MAX_TREE_DEPTH {
                return Err(TreeError::TooDeep(input.id.clone()));
            }
        }
        let mut nodes: BTreeMap<NodeId, Node> = inputs
            .iter()
            .map(|i| {
                (
                    i.id.clone(),
                    Node {
                        progress: i.progress,
                        ..Node::default()
                    },
                )
            })
            .collect();
        nodes.entry(NodeId::root()).or_default();
        for input in &inputs {
            link_children(&mut nodes, &input.id, &input.ref_tags, layout);
        }
        debug_assert!(nodes.len() <= MAX_TRACKED_REFS + 1, "root plus every input");
        Ok(WorkspaceTree { nodes })
    }

    /// This node's own rule-5 progress, if it is part of the tree.
    pub fn progress(&self, id: &NodeId) -> Option<Progress> {
        self.nodes.get(id).map(|n| n.progress)
    }

    /// The task whose `ref:` tag points at `id`; `None` for the root and for an orphan.
    pub fn owner(&self, id: &NodeId) -> Option<TaskId> {
        self.nodes.get(id)?.owner
    }

    /// `id`'s child directories, by slug, when `id` is part of the tree.
    pub fn children(&self, id: &NodeId) -> impl Iterator<Item = (&str, &NodeId)> {
        self.nodes
            .get(id)
            .into_iter()
            .flat_map(|n| n.children.iter().map(|(s, c)| (s.as_str(), c)))
    }

    /// Every node this tree holds, root included.
    pub fn nodes(&self) -> impl Iterator<Item = &NodeId> {
        self.nodes.keys()
    }

    /// Directories nothing points at (rule 10, `prune --orphans`): every non-root node with no
    /// owner.
    pub fn orphans(&self) -> impl Iterator<Item = &NodeId> {
        self.nodes
            .iter()
            .filter(|(id, n)| !id.is_root() && n.owner.is_none())
            .map(|(id, _)| id)
    }
}

/// For each of `parent`'s `ref:` tags whose target directory was actually discovered, records the
/// edge (in `parent`'s children map) and the target's owner. A tag naming an undiscovered
/// directory is a dangling edge (rule 9) and is silently skipped.
fn link_children(
    nodes: &mut BTreeMap<NodeId, Node>,
    parent: &NodeId,
    tags: &[RefTag],
    layout: &WorkspaceLayout,
) {
    for tag in tags {
        let Some(child) = parent.child(&tag.slug, layout) else {
            continue;
        };
        if !nodes.contains_key(&child) {
            continue; // dangling (rule 9): the tag exists, the directory does not
        }
        if let Some(node) = nodes.get_mut(&child) {
            node.owner.get_or_insert(tag.owner);
        }
        if let Some(node) = nodes.get_mut(parent) {
            node.children.insert(tag.slug.clone(), child);
        }
    }
}

/// Which cached values one op can invalidate (exhaustive over [`OpKind`] — see [`invalidates`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invalidation {
    /// Nodes whose rule-5 counters need recomputing (deduplicated by the caller if it cares; a
    /// cross-file `Move` names both the source and, when it differs, the destination).
    pub counters: Vec<NodeId>,
    /// Nodes whose `ref:` tag set (and therefore child edges) may have changed.
    pub edges: Vec<NodeId>,
}

impl Invalidation {
    fn counters_and_edges(id: NodeId) -> Invalidation {
        Invalidation {
            counters: vec![id.clone()],
            edges: vec![id],
        }
    }
    fn counters_only(id: NodeId) -> Invalidation {
        Invalidation {
            counters: vec![id],
            edges: Vec::new(),
        }
    }
    fn edges_only(id: NodeId) -> Invalidation {
        Invalidation {
            counters: Vec::new(),
            edges: vec![id],
        }
    }

    /// True when nothing needs recomputing.
    pub fn is_empty(&self) -> bool {
        self.counters.is_empty() && self.edges.is_empty()
    }
}

/// Which cached values `op` (recorded against `file`) can invalidate. Rule 5's counters and rule
/// 2's edges are cached separately, so this is precise rather than a blanket "recompute `file`'s
/// node": an op that cannot possibly move a `ref:` tag never triggers an edge rebuild, and one that
/// cannot change a task-line/completed count never triggers a counter one.
///
/// `Insert` and `Move`'s destination-side insert both carry the *full* new line as text, which can
/// already contain a hand-typed `ref:` tag (a plain `add "task ref:foo"`, or the line a `Move`
/// carries across); both are therefore also an edges invalidation, not just counters, which is
/// wider than a first reading of specs/ref-directories.md rule 5 suggests but is the correct
/// reading of rule 2 once a client-supplied `ref:` tag is considered.
///
/// Exhaustive over `OpKind`, no default arm — a new variant forces a decision here (CLAUDE.md §3).
pub fn invalidates(op: &OpKind, file: &FilePath) -> Invalidation {
    let node = NodeId::of_file(file);
    match op {
        OpKind::Insert { .. } => Invalidation::counters_and_edges(node),
        OpKind::SetField {
            field: Field::Completed,
            ..
        } => Invalidation::counters_only(node),
        // Removing a task line can remove the `ref:` tag it carried, turning its target into an
        // orphan (rule 10) — an edges change, not just a counters one.
        OpKind::SetField {
            field: Field::Deleted,
            ..
        } => Invalidation::counters_and_edges(node),
        OpKind::SetField {
            field: Field::Priority | Field::CreationDate | Field::CompletionDate | Field::Quirks,
            ..
        } => Invalidation::default(),
        // A description edit is the one that looks like it touches nothing structural but can add
        // or drop a `ref:` tag (see the module doc's rule 2/rule 5 split).
        OpKind::EditText { .. } => Invalidation::edges_only(node),
        OpKind::Move { to_file, .. } => {
            let dest = NodeId::of_file(to_file);
            if dest == node {
                Invalidation::counters_and_edges(node)
            } else {
                Invalidation {
                    counters: vec![node.clone(), dest.clone()],
                    edges: vec![node, dest],
                }
            }
        }
        // Blanks are excluded by rule 5 and carry no tags; a `notes.md` edit never touches a task
        // line at all (a different document kind entirely — see `txtodo-daemon`'s `NotesActor`).
        OpKind::NotesEdit { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. } => {
            Invalidation::default()
        }
    }
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
