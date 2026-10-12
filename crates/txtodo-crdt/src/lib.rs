//! The `notes.md` merge engine: [`NotesDoc`], one Loro text per notes file (plan M5). The op log
//! (`txtodo-store`) stays authoritative for persistence and transport; Loro is the in-memory
//! merge. The todo.txt list document and its op translation went with the daemon's mirror
//! (ADR 0038). Loro API: <https://docs.rs/loro>.
#![forbid(unsafe_code)]

mod notes;

pub use notes::{NotesDoc, NotesDocError};
