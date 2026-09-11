//! parse, tokenize, model, format, diff for todo.txt lines. Sync, no I/O, `no_std` + `alloc`.
//!
//! Grammar: `specs/todotxt.abnf` (normative). Design: `txtodo-design.md` §2–3. Plan: M1.
//! Every public item here is part of the frozen M1 API shape; details may grow, shapes may not shrink.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod file;
mod parse;
mod quirks;
mod scanner;
mod task;
mod tokenize;
mod types;
mod ulid;
pub mod urls;

pub use error::ParseError;
pub use file::{parse_file, File, OwnedLine};
pub use parse::{parse_line, parse_line_with_schemes};
pub use quirks::Quirks;
pub use task::{is_valid_slug, SLUG_MAX_LEN};
pub use tokenize::{tokenize, tokenize_with_schemes};
pub use types::{Date, Line, LineEnding, LineKind, Mode, Priority, Span, Task, TokenKind};
pub use ulid::{Ulid, ULID_LEN};
