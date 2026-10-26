//! The merge engine (ADR 0042): one document's state from its ops (`state`), a client's intent as
//! ops (`mutation`), a file edited outside txtodo as ops (`reconcile`, `reconcile_sidecar`), and a
//! task's identity across edits (`identity_*`). Pure: no I/O, no runtime, no clock read except
//! `local_date`. Moved out of `txtodo-daemon` so the daemon and the browser peer run the same code;
//! it builds for `wasm32-unknown-unknown`.
#![forbid(unsafe_code)]

pub mod apply;
pub mod conflict_row;
pub mod contents;
pub mod duplicates;
pub mod fastid;
pub mod fields;
pub mod id_strip;
pub mod identity_assign;
pub mod identity_fingerprint;
pub mod identity_levenshtein;
pub mod local_date;
pub mod mutation;
pub mod mutation_add;
pub mod mutation_moves;
pub mod mutation_reopen;
pub mod notes_state;
pub mod op_set_hash;
pub mod reconcile;
pub mod reconcile_replay;
pub mod reconcile_sidecar;
pub mod stamp_floor;
pub mod state;
pub mod state_error;
pub mod sync_park;
pub mod text_history;
pub mod textedit;
pub mod unified_diff;

#[cfg(test)]
mod conflict_table_tests;
#[cfg(test)]
mod mutation_reopen_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod mutation_tests_lines;
#[cfg(test)]
mod reconcile_sidecar_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod state_converge_pri_tests;
#[cfg(test)]
mod state_converge_tests;
#[cfg(test)]
mod state_goldens;
#[cfg(test)]
mod state_order_tests;
#[cfg(test)]
mod state_reinsert_tests;
#[cfg(test)]
mod state_split_group_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod sync_park_tests;
#[cfg(test)]
mod text_history_converge_tests;
#[cfg(test)]
mod unified_diff_tests;

/// A blake3 digest: a document's bytes (`hash_of`), as `GetFile` and `Apply`'s expected-hash
/// check carry it.
pub type Hash = [u8; 32];

/// blake3 of a projection.
/// Ref: <https://docs.rs/blake3/latest/blake3/fn.hash.html>
pub fn hash_of(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}
