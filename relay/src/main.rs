//! Reference relay binary (M8, design §4.5): stores encrypted op blobs by (group, device),
//! forwards push wake-ups. Deliberately has zero `txtodo-*` dependencies — see
//! tests/no_txtodo_deps.rs — the relay is untrusted and must never be able to parse
//! ciphertext it stores (design §4.6). Stub; full implementation is a separate task
//! (tasks/relay-reference/todo.txt).
#![forbid(unsafe_code)]

fn main() {}
