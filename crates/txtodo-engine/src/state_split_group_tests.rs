//! Lab chaos 20261011-144604, cause 2 (tasks/chaos-replay-order/notes.md): an older device's
//! `Move T` landed inside a newer device's commit group, between its `BlankInsert after T` and its
//! `BlankRemove`, on one device; before the group on another. Same 92 ops (cut down from the two
//! devices' logs), two orders, one blank line apart: the late move re-homed the commit's blocks
//! first-to-last, so a blank and a line of one commit swapped places (`state_rehome.rs`).

use crate::state::DocState;
use crate::sync_park::{Parked, apply_parking};
use txtodo_core::File;
use txtodo_model::{FilePath, IdentityMode, Op};

/// The two orders: (a1's, b1's).
const CASE: &str = include_str!("testdata/chaos_split_group_blank.json");

fn render(ops: &[Op]) -> String {
    let path = FilePath::new("todo.txt").unwrap();
    let mut state =
        DocState::from_file(path, &File::default(), &[], IdentityMode::Sidecar).unwrap();
    apply_parking(&mut state, ops, &mut Parked::default());
    String::from_utf8(state.to_bytes()).unwrap()
}

#[test]
fn an_older_move_inside_a_newer_groups_blank_ops_renders_the_same() {
    let (a1, b1): (Vec<Op>, Vec<Op>) = serde_json::from_str(CASE).unwrap();
    assert_eq!(a1.len(), b1.len());
    assert_eq!(render(&a1), render(&b1));
}
