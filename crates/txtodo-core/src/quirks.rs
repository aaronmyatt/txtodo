//! Leniencies recorded while reading, so a line round-trips byte-for-byte (design §2.3).

use core::fmt;

/// A set of quirks seen on one line. Bits are never reused; new quirks get new bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Quirks(u16);

impl Quirks {
    /// No quirks.
    pub const NONE: Quirks = Quirks(0);
    /// `x` followed by something that is not a date.
    pub const NO_COMPLETION_DATE: Quirks = Quirks(1 << 0);
    /// `x (A) 2026-…`: priority right after the marker.
    pub const PRIORITY_AFTER_X: Quirks = Quirks(1 << 1);
    /// `x 2026-… (A) …`: priority after the completion date.
    pub const PRIORITY_AFTER_DATE: Quirks = Quirks(1 << 2);
    /// Tabs or runs of spaces separate words.
    pub const TABS: Quirks = Quirks(1 << 3);
    /// Whitespace after the last word.
    pub const TRAILING_WS: Quirks = Quirks(1 << 4);
    /// A `ref:` tag whose value is not a valid slug (plan §3.2.1); treated as no ref.
    pub const INVALID_REF: Quirks = Quirks(1 << 5);
    /// This line's ending differs from the file's dominant ending.
    pub const MIXED_ENDING: Quirks = Quirks(1 << 6);

    /// All named quirks with their lint names, in bit order.
    pub const ALL: [(Quirks, &'static str); 7] = [
        (Quirks::NO_COMPLETION_DATE, "no_completion_date"),
        (Quirks::PRIORITY_AFTER_X, "priority_after_x"),
        (Quirks::PRIORITY_AFTER_DATE, "priority_after_date"),
        (Quirks::TABS, "tabs"),
        (Quirks::TRAILING_WS, "trailing_ws"),
        (Quirks::INVALID_REF, "invalid_ref"),
        (Quirks::MIXED_ENDING, "mixed_ending"),
    ];

    /// True when every bit of `other` is set in `self`.
    pub fn has(self, other: Quirks) -> bool {
        self.0 & other.0 == other.0
    }
    /// Adds the bits of `other`.
    pub fn insert(&mut self, other: Quirks) {
        self.0 |= other.0;
    }
    /// Removes the bits of `other`.
    pub fn remove(&mut self, other: Quirks) {
        self.0 &= !other.0;
    }
    /// True when no quirk is set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// The lint names of the set quirks, in bit order.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        Quirks::ALL.into_iter().filter(move |(q, _)| self.has(*q)).map(|(_, n)| n)
    }
}

impl core::ops::BitOr for Quirks {
    type Output = Quirks;
    fn bitor(self, rhs: Quirks) -> Quirks {
        Quirks(self.0 | rhs.0)
    }
}

impl fmt::Display for Quirks {
    /// Comma-separated lint names, or `none`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("none");
        }
        for (i, name) in self.names().enumerate() {
            let sep = if i == 0 { "" } else { "," };
            write!(f, "{sep}{name}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_has_remove_and_names() {
        let mut q = Quirks::NONE;
        assert!(q.is_empty());
        q.insert(Quirks::TABS | Quirks::INVALID_REF);
        assert!(q.has(Quirks::TABS) && q.has(Quirks::INVALID_REF) && !q.has(Quirks::TRAILING_WS));
        assert_eq!(alloc::format!("{q}"), "tabs,invalid_ref");
        q.remove(Quirks::TABS);
        assert_eq!(q, Quirks::INVALID_REF);
        assert_eq!(alloc::format!("{}", Quirks::NONE), "none");
    }
}
