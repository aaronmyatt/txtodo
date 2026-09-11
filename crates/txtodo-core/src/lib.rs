//! parse, tokenize, model, format, diff for todo.txt lines. Sync, no I/O, `no_std` + `alloc`.
//!
//! Grammar: `specs/todotxt.abnf` (normative). Design: `txtodo-design.md` §2–3. Plan: M1.
//! Every public item here is part of the frozen M1 API shape; details may grow, shapes may not shrink.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod file;
mod quirks;
mod types;
mod ulid;

pub use error::ParseError;
pub use file::{File, OwnedLine};
pub use quirks::Quirks;
pub use types::{Date, Line, LineEnding, LineKind, Mode, Priority, Span, Task, TokenKind};
pub use ulid::{Ulid, ULID_LEN};
