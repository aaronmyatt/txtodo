//! The mutation API: build an [`Edit`], [`apply`] it to an [`OwnedLine`], get a new line whose untouched
//! bytes are identical. `complete`/`uncomplete` implement the spec's `pri:` rule.

use crate::format::{Prefix, description_start, emit_prefix};
use crate::scanner::chunks;
use crate::tokenize::{WordKind, classify_word};
use crate::urls::DEFAULT_SCHEMES;
use crate::{Date, LineKind, Mode, OwnedLine, Priority};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

/// A rejected edit argument. Checked when the edit is built, so callers learn before applying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    /// Text contained a line break.
    LineBreak,
    /// A tag key or value was empty, contained whitespace, or the key contained a colon.
    InvalidTag,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EditError::LineBreak => "text must not contain a line break",
            EditError::InvalidTag => {
                "tag keys and values are non-empty, without whitespace; keys have no colon"
            }
        })
    }
}

#[cfg(feature = "std")]
impl std::error::Error for EditError {}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Op {
    SetPriority(Priority),
    ClearPriority,
    SetDescription(String),
    SetTag(String, String),
    RemoveTag(String),
    Append(String),
    Prepend(String),
    Complete(Date),
    Uncomplete,
}

/// An ordered list of field-level mutations. Build with the methods, then [`apply`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    ops: Vec<Op>,
}

fn no_line_break(s: &str) -> Result<(), EditError> {
    if s.contains(['\n', '\r']) {
        Err(EditError::LineBreak)
    } else {
        Ok(())
    }
}

fn valid_tag(key: &str, value: &str) -> Result<(), EditError> {
    let word_ok = |s: &str| !s.is_empty() && !s.bytes().any(|b| b.is_ascii_whitespace());
    if word_ok(key) && word_ok(value) && !key.contains(':') {
        Ok(())
    } else {
        Err(EditError::InvalidTag)
    }
}

impl Edit {
    /// No ops; applying it is the identity.
    pub fn new() -> Edit {
        Edit::default()
    }
    /// True when no op was added.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
    /// Set `(P)`. On a completed line the priority becomes a `pri:P` tag when formatted.
    pub fn set_priority(mut self, p: Priority) -> Edit {
        self.ops.push(Op::SetPriority(p));
        self
    }
    /// Remove the priority.
    pub fn clear_priority(mut self) -> Edit {
        self.ops.push(Op::ClearPriority);
        self
    }
    /// Replace the whole description.
    pub fn set_description(mut self, text: &str) -> Result<Edit, EditError> {
        no_line_break(text)?;
        self.ops.push(Op::SetDescription(text.to_string()));
        Ok(self)
    }
    /// Set `key:value`, replacing the first existing `key:` in place or appending at the end.
    pub fn set_tag(mut self, key: &str, value: &str) -> Result<Edit, EditError> {
        valid_tag(key, value)?;
        self.ops
            .push(Op::SetTag(key.to_string(), value.to_string()));
        Ok(self)
    }
    /// Remove the first `key:value` word and one adjacent space.
    pub fn remove_tag(mut self, key: &str) -> Result<Edit, EditError> {
        valid_tag(key, "v")?;
        self.ops.push(Op::RemoveTag(key.to_string()));
        Ok(self)
    }
    /// Add ` text` at the end of the description.
    pub fn append(mut self, text: &str) -> Result<Edit, EditError> {
        no_line_break(text)?;
        self.ops.push(Op::Append(text.to_string()));
        Ok(self)
    }
    /// Add `text ` at the start of the description.
    pub fn prepend(mut self, text: &str) -> Result<Edit, EditError> {
        no_line_break(text)?;
        self.ops.push(Op::Prepend(text.to_string()));
        Ok(self)
    }
    /// Mark done on `today`; a priority moves to `pri:P` (todo.txt spec). Idempotent.
    pub fn complete(mut self, today: Date) -> Edit {
        self.ops.push(Op::Complete(today));
        self
    }
    /// Reopen; a `pri:P` tag becomes the priority again. Idempotent.
    pub fn uncomplete(mut self) -> Edit {
        self.ops.push(Op::Uncomplete);
        self
    }
}

/// The task under edit: an owned prefix plus an owned description.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    prefix: Prefix,
    description: String,
}

impl Draft {
    fn apply(&mut self, op: &Op) {
        match op {
            Op::SetPriority(p) => self.prefix.priority = Some(*p),
            Op::ClearPriority => self.prefix.priority = None,
            Op::SetDescription(s) => self.description = s.clone(),
            Op::SetTag(k, v) => self.set_tag(k, v),
            Op::RemoveTag(k) => self.remove_tag(k),
            Op::Append(s) => self.append(s),
            Op::Prepend(s) => self.prepend(s),
            Op::Complete(today) => self.complete(*today),
            Op::Uncomplete => self.uncomplete(),
        }
    }

    /// Byte range of the first `key:` tag word in the description.
    fn find_tag(&self, key: &str) -> Option<(usize, usize)> {
        let d = self.description.as_str();
        chunks(d).filter(|c| !c.is_ws).find_map(|c| {
            match classify_word(&d[c.start..c.end], DEFAULT_SCHEMES) {
                WordKind::Tag(colon) if &d[c.start..c.start + colon] == key => {
                    Some((c.start, c.end))
                }
                WordKind::IdTag if key == "id" => Some((c.start, c.end)),
                _ => None,
            }
        })
    }

    fn set_tag(&mut self, key: &str, value: &str) {
        let word = alloc::format!("{key}:{value}");
        match self.find_tag(key) {
            Some((start, end)) => self.description.replace_range(start..end, &word),
            None => self.append(&word),
        }
        debug_assert!(self.description.contains(&word), "tag present after set");
    }

    /// Removes the word and the space before it (or after it when it is first).
    fn remove_tag(&mut self, key: &str) {
        let Some((start, end)) = self.find_tag(key) else {
            return;
        };
        let d = &self.description;
        let (from, to) = if start > 0 && d.as_bytes()[start - 1] == b' ' {
            (start - 1, end)
        } else if d.as_bytes().get(end) == Some(&b' ') {
            (start, end + 1)
        } else {
            (start, end)
        };
        debug_assert!(from <= start && to >= end, "removal covers the word");
        self.description.replace_range(from..to, "");
    }

    fn append(&mut self, text: &str) {
        if !self.description.is_empty() && !text.is_empty() {
            self.description.push(' ');
        }
        self.description.push_str(text);
    }

    fn prepend(&mut self, text: &str) {
        if self.description.is_empty() || text.is_empty() {
            self.description.insert_str(0, text);
            return;
        }
        self.description.insert(0, ' ');
        self.description.insert_str(0, text);
    }

    fn complete(&mut self, today: Date) {
        if self.prefix.completed {
            return;
        }
        self.prefix.completed = true;
        self.prefix.completion_date = Some(today);
        if let Some(p) = self.prefix.priority.take() {
            self.set_tag("pri", p.as_char().encode_utf8(&mut [0; 4]));
        }
        debug_assert!(self.prefix.priority.is_none(), "priority moved to pri:");
    }

    fn uncomplete(&mut self) {
        if !self.prefix.completed {
            return;
        }
        self.prefix.completed = false;
        self.prefix.completion_date = None;
        let restored = self
            .find_tag("pri")
            .and_then(|(s, e)| self.description[s + 4..e].chars().next())
            .and_then(Priority::new);
        if let Some(p) = restored {
            self.prefix.priority = Some(p);
            self.remove_tag("pri");
        }
    }
}

/// Applies `edit` and returns a new line. Untouched bytes are identical: an unchanged prefix or description
/// is spliced from the original, so quirks in the part you did not edit survive. An opaque (non-UTF-8) line
/// is returned unchanged. The line ending is always kept.
pub fn apply(line: &OwnedLine, edit: &Edit) -> OwnedLine {
    let Some(raw) = line.raw() else {
        return line.clone();
    };
    let original = draft_of(raw);
    let mut draft = original.clone();
    for op in &edit.ops {
        draft.apply(op);
    }
    let prefix_dirty = draft.prefix != original.prefix;
    if prefix_dirty && draft.prefix.completed && draft.prefix.priority.is_some() {
        // strict grammar has no priority slot after `x`: keep it as data (pri:), never lose it.
        // Only when the prefix is being rewritten; a lenient `x (A) …` left alone stays byte-identical.
        let p = draft.prefix.priority.take().map_or('A', Priority::as_char);
        draft.set_tag("pri", p.encode_utf8(&mut [0; 4]));
    }
    let description_dirty = draft.description != original.description;
    if !prefix_dirty && !description_dirty {
        return line.clone();
    }
    let split = description_start(raw);
    let description = if description_dirty {
        draft.description.as_str()
    } else {
        &raw[split..]
    };
    let out = if prefix_dirty {
        emit_prefix(&draft.prefix, !description.is_empty()) + description
    } else {
        join(&raw[..split], description)
    };
    debug_assert!(!out.contains('\n'), "edits never introduce line breaks");
    OwnedLine::from_bytes(out.into_bytes(), line.ending())
}

/// Splices an original prefix slice and a description with exactly one separator between them when both
/// are non-empty, and none when the description is empty. Whitespace already inside the prefix slice
/// (a tab quirk, say) is kept.
fn join(prefix: &str, description: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + description.len() + 1);
    out.push_str(prefix);
    let ends_ws = out.bytes().last().is_some_and(|b| b == b' ' || b == b'\t');
    if description.is_empty() {
        while out.bytes().last().is_some_and(|b| b == b' ' || b == b'\t') {
            out.pop();
        }
    } else if !out.is_empty() && !ends_ws {
        out.push(' ');
    }
    out.push_str(description);
    debug_assert!(
        description.is_empty() || out.ends_with(description),
        "description is the tail"
    );
    out
}

fn draft_of(raw: &str) -> Draft {
    match crate::parse_line(raw, Mode::Lenient).map(|l| l.kind) {
        Ok(LineKind::Task(t)) => Draft {
            prefix: Prefix::of(&t),
            description: t.description.to_string(),
        },
        _ => Draft {
            prefix: Prefix::default(),
            description: String::new(),
        },
    }
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
