//! `reconcile_replay.rs`'s free functions, as the web host calls them (ADR 0042): a whole new text
//! reconciled against a sidecar document and settled lands on that text, ids kept. The daemon's
//! `reconcile_replay_tests.rs` covers the synthesis cases through `FileActor`.

use super::*;
use crate::state::DocState;
use txtodo_core::parse_file;
use txtodo_model::Ulid;

fn sidecar(text: &str, ids: &[u128]) -> DocState {
    let file = parse_file(text.as_bytes());
    let ids: Vec<Option<TaskId>> = ids
        .iter()
        .map(|&n| (n != 0).then(|| TaskId::new(Ulid::from_u128(1 << 80 | n))))
        .collect();
    let path = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
    DocState::from_file(path, &file, &ids, IdentityMode::Sidecar)
        .unwrap_or_else(|e| panic!("doc: {e}"))
}

/// A reorder plus an edit: the settled state renders the new text and keeps both ids.
#[test]
fn a_sidecar_text_settles_to_itself_with_ids_kept() {
    let state = sidecar("call mum\nbuy milk\n", &[1, 2]);
    let old = parse_file(&state.to_bytes());
    let new = parse_file(b"buy milk and eggs\ncall mum\n");
    let mut minted = 0;
    let mut mint = || {
        minted += 1;
        TaskId::new(Ulid::from_u128(9 << 80 | minted))
    };
    let r = reconcile_against(&state, &old, &new, &mut mint);
    let settled = settle(&state, r).unwrap_or_else(|e| panic!("settle: {e}"));
    assert!(settled.exact);
    assert_eq!(settled.next.to_bytes(), b"buy milk and eggs\ncall mum\n");
    let ids: Vec<_> = (0..2)
        .map(|i| settled.next.entry_at(i).and_then(|e| e.id()))
        .collect();
    assert_eq!(
        ids,
        [
            state.entry_at(1).and_then(|e| e.id()),
            state.entry_at(0).and_then(|e| e.id())
        ]
    );
    assert_eq!(minted, 0, "no new id for an edited line");
}
