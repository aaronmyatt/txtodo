//! Loro document, ops to/from Loro, reconciler (plan M4).
//!
//! The op log (`txtodo-store`) stays authoritative for persistence and transport; Loro is the
//! in-memory merge engine. This crate is a pure translation layer plus a document shape:
//! `to_loro` applies one [`txtodo_model::Op`] to a [`LoroDocument`], `from_loro` turns Loro
//! diff data back into [`txtodo_model::Op`]s. Loro API: <https://docs.rs/loro>.
#![forbid(unsafe_code)]

mod doc;
mod from_loro;
mod lww;
mod to_loro;

#[cfg(test)]
mod lww_tests;
#[cfg(test)]
mod roundtrip_tests;
#[cfg(test)]
mod view_tests;

pub use doc::{LoroDocument, is_blank, rebuild_line};
pub use from_loro::{FromLoroError, Stamp, from_batch};
pub use lww::{Lww, write_if_newer};
pub use to_loro::{ToLoroError, apply};
