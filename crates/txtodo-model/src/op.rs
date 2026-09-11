//! The op model (plan M3, verbatim shapes). One op = one change to one document by one principal,
//! stamped with an [`Hlc`]. `txtodo-store` persists them, the daemon derives and applies them, M4
//! maps them onto Loro. `TextEdit` mirrors `txtodo_core::TextEdit` because core has no serde.

use crate::{DeviceId, FilePath, Hlc, OpId, TaskId, TokenId};
use core::fmt;
use serde::{Deserialize, Serialize};
use txtodo_core::{Date, Priority, Quirks};

/// One recorded change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    /// Unique id.
    pub id: OpId,
    /// When, in HLC order.
    pub hlc: Hlc,
    /// Who.
    pub principal: Principal,
    /// Which document.
    pub file: FilePath,
    /// What.
    pub kind: OpKind,
}

/// What an op does. Closed set; every `match` over it is exhaustive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpKind {
    /// A new task line placed after `after` (`None` = at the top).
    Insert {
        /// The new task's id.
        task: TaskId,
        /// Predecessor, or `None` for the first line.
        after: Option<TaskId>,
        /// The full line bytes as text, `id:` tag included.
        line: String,
    },
    /// A prefix field or the deleted flag changed.
    SetField {
        /// Which task.
        task: TaskId,
        /// Which field; paired with the value by [`SetField::new`].
        field: Field,
        /// The new value.
        value: FieldValue,
    },
    /// The description text changed (tags are text too, design §2.3).
    EditText {
        /// Which task.
        task: TaskId,
        /// Char-level edits against the previous description.
        edits: Vec<TextEdit>,
    },
    /// The task moved, within a file or to another one.
    Move {
        /// Which task.
        task: TaskId,
        /// New predecessor in `to_file`.
        after: Option<TaskId>,
        /// Destination document.
        to_file: FilePath,
    },
    /// A `notes.md` edit (M5).
    NotesEdit {
        /// The notes document.
        file: FilePath,
        /// Char-level edits.
        edits: Vec<TextEdit>,
    },
    /// A blank line inserted after `after`.
    BlankInsert {
        /// Predecessor task, or `None` for the top.
        after: Option<TaskId>,
    },
    /// A blank line removed after `after`.
    BlankRemove {
        /// Predecessor task, or `None` for the top.
        after: Option<TaskId>,
    },
}

/// Prefix fields and flags a `SetField` can target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Field {
    /// `x` marker.
    Completed,
    /// Date after `x`.
    CompletionDate,
    /// Creation date.
    CreationDate,
    /// `(A)`–`(Z)`.
    Priority,
    /// Tombstone.
    Deleted,
    /// Lenient-mode quirks kept for byte fidelity.
    Quirks,
}

/// A field's new value. Core types are carried as their raw representation (core has no serde).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldValue {
    /// For `Completed` and `Deleted`.
    Bool(bool),
    /// For the two dates: `(year, month, day)`, or `None` to clear.
    Date(Option<(u16, u8, u8)>),
    /// For `Priority`: the letter, or `None` to clear.
    Priority(Option<char>),
    /// For `Quirks`: the flag names that are set, in `Quirks::names` order — stable across versions.
    Quirks(u16),
}

/// Why a `SetField` pair was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMismatch {
    /// The field.
    pub field: Field,
}

impl fmt::Display for FieldMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "value does not fit field {:?}", self.field)
    }
}

impl std::error::Error for FieldMismatch {}

/// Builds `OpKind::SetField` after checking the field/value pairing, so a mismatched pair cannot exist.
pub fn set_field(task: TaskId, field: Field, value: FieldValue) -> Result<OpKind, FieldMismatch> {
    let ok = matches!(
        (field, value),
        (Field::Completed | Field::Deleted, FieldValue::Bool(_))
            | (
                Field::CompletionDate | Field::CreationDate,
                FieldValue::Date(_)
            )
            | (Field::Priority, FieldValue::Priority(_))
            | (Field::Quirks, FieldValue::Quirks(_))
    );
    if !ok {
        return Err(FieldMismatch { field });
    }
    Ok(OpKind::SetField { task, field, value })
}

impl FieldValue {
    /// A date value.
    pub fn date(d: Option<Date>) -> FieldValue {
        FieldValue::Date(d.map(|d| (d.year(), d.month(), d.day())))
    }
    /// A priority value.
    pub fn priority(p: Option<Priority>) -> FieldValue {
        FieldValue::Priority(p.map(Priority::as_char))
    }
    /// A quirks value, encoded as a bitmask in the order `Quirks::names` lists the set flags.
    pub fn quirks(q: Quirks) -> FieldValue {
        let bits = ALL_QUIRKS
            .iter()
            .enumerate()
            .filter(|(_, f)| q.has(**f))
            .fold(0u16, |m, (i, _)| m | (1 << i));
        debug_assert!(bits.count_ones() as usize <= ALL_QUIRKS.len());
        FieldValue::Quirks(bits)
    }
    /// The date back, if this is a date value with a valid calendar date.
    pub fn as_date(self) -> Option<Option<Date>> {
        match self {
            FieldValue::Date(None) => Some(None),
            FieldValue::Date(Some((y, m, d))) => Date::new(y, m, d).map(Some),
            _ => None,
        }
    }
    /// The quirks back, if this is a quirks value.
    pub fn as_quirks(self) -> Option<Quirks> {
        let FieldValue::Quirks(bits) = self else {
            return None;
        };
        let mut q = Quirks::NONE;
        for (i, f) in ALL_QUIRKS.iter().enumerate() {
            if bits & (1 << i) != 0 {
                q.insert(*f);
            }
        }
        Some(q)
    }
}

/// Every quirk flag, in a fixed order that the `Quirks(u16)` encoding depends on. Append only.
const ALL_QUIRKS: [Quirks; 8] = [
    Quirks::NO_COMPLETION_DATE,
    Quirks::PRIORITY_AFTER_X,
    Quirks::PRIORITY_AFTER_DATE,
    Quirks::TABS,
    Quirks::TRAILING_WS,
    Quirks::INVALID_REF,
    Quirks::MIXED_ENDING,
    Quirks::LEADING_WS,
];

/// Char-level text edit; same shape as `txtodo_core::TextEdit`, with serde.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextEdit {
    /// Insert `text` so that it starts at char index `at` of the result (`diff_text` convention).
    Insert {
        /// Char index in the target text.
        at: usize,
        /// Chars to insert.
        text: String,
    },
    /// Delete `len` chars starting at char index `at`.
    Delete {
        /// Char index in the original.
        at: usize,
        /// Chars to delete.
        len: usize,
    },
}

impl From<txtodo_core::TextEdit> for TextEdit {
    fn from(e: txtodo_core::TextEdit) -> TextEdit {
        match e {
            txtodo_core::TextEdit::Insert { at, text } => TextEdit::Insert { at, text },
            txtodo_core::TextEdit::Delete { at, len } => TextEdit::Delete { at, len },
        }
    }
}

impl From<TextEdit> for txtodo_core::TextEdit {
    fn from(e: TextEdit) -> txtodo_core::TextEdit {
        match e {
            TextEdit::Insert { at, text } => txtodo_core::TextEdit::Insert { at, text },
            TextEdit::Delete { at, len } => txtodo_core::TextEdit::Delete { at, len },
        }
    }
}

/// Who made a change (design §4.8 shows it in `txtodo log`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Principal {
    /// A person, through a client of this device.
    User {
        /// The device.
        device: DeviceId,
    },
    /// An agent through the MCP surface (M6).
    Agent {
        /// Its token.
        token_id: TokenId,
        /// Its display name.
        name: String,
        /// The device.
        device: DeviceId,
    },
    /// An edit seen on disk that no client made through the daemon.
    External {
        /// The device.
        device: DeviceId,
    },
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Principal::User { device } => write!(f, "you@{device}"),
            Principal::Agent { name, device, .. } => write!(f, "agent:{name}@{device}"),
            Principal::External { device } => write!(f, "external@{device}"),
        }
    }
}
