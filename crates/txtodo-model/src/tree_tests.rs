//! Unit and property tests for `tree.rs`. Sibling file per this crate's `hlc_tests.rs`/`op_tests.rs`
//! convention (declared with `#[path = ...]` in the module under test).

use super::*;
use crate::{FieldValue, Ulid, set_field};
use proptest::prelude::*;

fn path(s: &str) -> FilePath {
    FilePath::new(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn tag(owner: TaskId, slug: &str) -> RefTag {
    RefTag {
        owner,
        slug: slug.to_owned(),
    }
}

#[test]
fn node_id_of_file_is_the_dirname_or_the_root() {
    assert_eq!(NodeId::of_file(&path("todo.txt")), NodeId::root());
    assert_eq!(
        NodeId::of_file(&path("buy-ducks/todo.txt")),
        NodeId::dir(path("buy-ducks"))
    );
    assert_eq!(
        NodeId::of_file(&path("buy-ducks/sync/done.txt")),
        NodeId::dir(path("buy-ducks/sync"))
    );
    assert_eq!(NodeId::root().depth(), 0);
    assert_eq!(NodeId::dir(path("a")).depth(), 1);
    assert_eq!(NodeId::dir(path("a/b/c")).depth(), 3);
}

#[test]
fn build_links_a_tag_to_its_discovered_child_and_leaves_the_root_synthesised() {
    let owner = task(1);
    let root = NodeInput {
        id: NodeId::root(),
        progress: Progress { done: 1, total: 2 },
        ref_tags: vec![tag(owner, "buy-ducks")],
    };
    let child = NodeInput {
        id: NodeId::dir(path("buy-ducks")),
        progress: Progress { done: 0, total: 3 },
        ref_tags: vec![],
    };
    let tree = WorkspaceTree::build(vec![root, child]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(tree.nodes().count(), 2);
    assert_eq!(
        tree.progress(&NodeId::dir(path("buy-ducks"))),
        Some(Progress { done: 0, total: 3 })
    );
    assert_eq!(tree.owner(&NodeId::dir(path("buy-ducks"))), Some(owner));
    let kids: Vec<(&str, &NodeId)> = tree.children(&NodeId::root()).collect();
    assert_eq!(kids, vec![("buy-ducks", &NodeId::dir(path("buy-ducks")))]);
    assert!(tree.orphans().next().is_none());
}

#[test]
fn a_ref_tag_with_no_directory_is_dangling_not_an_error() {
    let root = NodeInput {
        id: NodeId::root(),
        progress: Progress::default(),
        ref_tags: vec![tag(task(1), "nowhere")],
    };
    let tree = WorkspaceTree::build(vec![root]).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(tree.children(&NodeId::root()).next().is_none());
    assert_eq!(tree.progress(&NodeId::dir(path("nowhere"))), None);
}

#[test]
fn a_directory_nothing_points_at_is_an_orphan() {
    let root = NodeInput {
        id: NodeId::root(),
        progress: Progress::default(),
        ref_tags: vec![],
    };
    let orphan = NodeInput {
        id: NodeId::dir(path("stray")),
        progress: Progress::default(),
        ref_tags: vec![],
    };
    let tree = WorkspaceTree::build(vec![root, orphan]).unwrap_or_else(|e| panic!("{e:?}"));
    let found: Vec<&NodeId> = tree.orphans().collect();
    assert_eq!(found, vec![&NodeId::dir(path("stray"))]);
    // The root itself is never an orphan even though nothing "points at" it either.
    assert!(!found.contains(&&NodeId::root()));
}

#[test]
fn nesting_past_max_tree_depth_is_refused() {
    let mut deep = String::new();
    for i in 0..=MAX_TREE_DEPTH {
        if !deep.is_empty() {
            deep.push('/');
        }
        deep.push_str(&format!("d{i}"));
    }
    let input = NodeInput {
        id: NodeId::dir(path(&deep)),
        progress: Progress::default(),
        ref_tags: vec![],
    };
    assert!(matches!(
        WorkspaceTree::build(vec![input]),
        Err(TreeError::TooDeep(_))
    ));
}

#[test]
fn more_than_max_tracked_refs_is_refused() {
    let inputs: Vec<NodeInput> = (0..=MAX_TRACKED_REFS)
        .map(|i| NodeInput {
            id: NodeId::dir(path(&format!("d{i}"))),
            progress: Progress::default(),
            ref_tags: vec![],
        })
        .collect();
    assert!(matches!(
        WorkspaceTree::build(inputs),
        Err(TreeError::TooMany(_))
    ));
}

#[test]
fn insert_invalidates_both_counters_and_edges() {
    let f = path("buy-ducks/todo.txt");
    let node = NodeId::dir(path("buy-ducks"));
    let insert = OpKind::Insert {
        task: task(1),
        after: None,
        line: "x ref:sub".into(),
    };
    let inv = invalidates(&insert, &f);
    assert_eq!(inv.counters, vec![node.clone()]);
    assert_eq!(inv.edges, vec![node]);
}

#[test]
fn completed_invalidates_counters_only() {
    let f = path("buy-ducks/todo.txt");
    let node = NodeId::dir(path("buy-ducks"));
    let complete = set_field(task(1), Field::Completed, FieldValue::Bool(true)).unwrap();
    let inv = invalidates(&complete, &f);
    assert_eq!(inv.counters, vec![node]);
    assert!(inv.edges.is_empty());
}

#[test]
fn deleted_invalidates_both_counters_and_edges() {
    let f = path("buy-ducks/todo.txt");
    let node = NodeId::dir(path("buy-ducks"));
    let deleted = set_field(task(1), Field::Deleted, FieldValue::Bool(true)).unwrap();
    let inv = invalidates(&deleted, &f);
    assert_eq!(inv.counters, vec![node.clone()]);
    assert_eq!(inv.edges, vec![node]);
}

#[test]
fn priority_and_other_prefix_fields_invalidate_nothing() {
    let f = path("buy-ducks/todo.txt");
    let priority = set_field(task(1), Field::Priority, FieldValue::Priority(Some('A'))).unwrap();
    assert!(invalidates(&priority, &f).is_empty());
}

#[test]
fn edit_text_invalidates_edges_only() {
    let f = path("buy-ducks/todo.txt");
    let node = NodeId::dir(path("buy-ducks"));
    let edit = OpKind::EditText {
        task: task(1),
        edits: vec![],
    };
    let inv = invalidates(&edit, &f);
    assert!(inv.counters.is_empty());
    assert_eq!(inv.edges, vec![node]);
}

#[test]
fn blanks_and_notes_invalidate_nothing() {
    let f = path("buy-ducks/todo.txt");
    assert!(invalidates(&OpKind::BlankInsert { after: None }, &f).is_empty());
    assert!(invalidates(&OpKind::BlankRemove { after: None }, &f).is_empty());
    let notes = OpKind::NotesEdit {
        file: f.clone(),
        edits: vec![],
    };
    assert!(invalidates(&notes, &f).is_empty());
}

#[test]
fn a_cross_file_move_invalidates_both_ends_a_same_file_move_only_one() {
    let source = path("buy-ducks/todo.txt");
    let dest = path("other-ref/todo.txt");
    let mv = OpKind::Move {
        task: task(1),
        after: None,
        to_file: dest.clone(),
    };
    let inv = invalidates(&mv, &source);
    assert_eq!(
        inv.counters,
        vec![NodeId::of_file(&source), NodeId::of_file(&dest)]
    );
    assert_eq!(
        inv.edges,
        vec![NodeId::of_file(&source), NodeId::of_file(&dest)]
    );

    let reorder = OpKind::Move {
        task: task(1),
        after: None,
        to_file: source.clone(),
    };
    let inv = invalidates(&reorder, &source);
    assert_eq!(inv.counters, vec![NodeId::of_file(&source)]);
    assert_eq!(inv.edges, vec![NodeId::of_file(&source)]);
}

proptest! {
    /// Every node `build` produces is reachable by its own path rule: a child is always strictly
    /// deeper than its parent, so the graph this module builds can never contain a cycle no matter
    /// what slugs the fuzzer throws at it (the invariant the module doc claims "structurally
    /// impossible").
    #[test]
    fn children_are_always_deeper_than_their_parent(
        slugs in prop::collection::vec("[a-z][a-z0-9-]{0,10}", 0..8),
    ) {
        let root_tags: Vec<RefTag> = slugs
            .iter()
            .enumerate()
            .map(|(i, s)| tag(task(i as u128 + 1), s))
            .collect();
        let mut inputs = vec![NodeInput {
            id: NodeId::root(),
            progress: Progress::default(),
            ref_tags: root_tags.clone(),
        }];
        for s in &slugs {
            inputs.push(NodeInput {
                id: NodeId::dir(path(s)),
                progress: Progress::default(),
                ref_tags: vec![],
            });
        }
        if let Ok(tree) = WorkspaceTree::build(inputs) {
            for (_, child) in tree.children(&NodeId::root()) {
                prop_assert!(child.depth() > NodeId::root().depth());
            }
        }
    }
}
