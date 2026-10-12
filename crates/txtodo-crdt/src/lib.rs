//! Loro document, ops to/from Loro, reconciler (plan M4).
//!
//! The op log (`txtodo-store`) stays authoritative for persistence and transport; Loro is the
//! in-memory merge engine. This crate is a pure translation layer plus a document shape:
//! `to_loro` applies one [`txtodo_model::Op`] to a [`LoroDocument`], `from_loro` turns Loro
//! diff data back into [`txtodo_model::Op`]s. Loro API: <https://docs.rs/loro>.
#![forbid(unsafe_code)]

mod doc;
mod lww;
mod notes;

pub use doc::{LoroDocument, is_blank};
pub use lww::{Lww, write_if_newer};
pub use notes::{NotesDoc, NotesDocError};
