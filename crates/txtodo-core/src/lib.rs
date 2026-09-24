//! parse, tokenize, model, format, diff for todo.txt lines. Sync, no I/O, `no_std` + `alloc`.
//!
//! Grammar: `specs/todotxt.abnf` (normative). Design: `txtodo-design.md` §2–3. Plan: M1.
//! Every public item here is part of the frozen M1 API shape; details may grow, shapes may not shrink.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod diff;
mod edit;
mod error;
mod file;
mod format;
mod line_length;
mod lint;
mod parse;
pub mod query;
mod quirks;
mod scanner;
mod task;
mod tokenize;
mod types;
mod ulid;
pub mod urls;

pub use diff::{LineDiff, TextEdit, diff_lines, diff_text};
pub use edit::{Edit, EditError, apply};
pub use error::ParseError;
pub use file::{File, OwnedLine, parse_file};
pub use format::{Prefix, description_start, emit_prefix};
pub use line_length::{LINE_LENGTH_HINT, over_length_hint, visible_chars};
pub use lint::findings as lint_findings;
pub use parse::{parse_line, parse_line_with_schemes};
pub use quirks::Quirks;
pub use task::{SLUG_MAX_LEN, is_valid_slug};
pub use tokenize::{tokenize, tokenize_with_schemes};
pub use types::{Date, Line, LineEnding, LineKind, Mode, Priority, Span, Task, TokenKind};
pub use ulid::{ULID_LEN, Ulid};
