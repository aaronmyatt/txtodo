//! The Loro merge engine for `notes.md` (plan M5, ADR 0002): [`NotesDoc`], one `LoroText` per
//! notes document. The todo.txt Loro mirror this crate also held, with its op translation, review
//! flags and simulator, went with ADR 0038: nothing read it. Loro API: <https://docs.rs/loro>.
#![forbid(unsafe_code)]

mod notes;
mod text;

pub use notes::{NotesDoc, NotesDocError};
