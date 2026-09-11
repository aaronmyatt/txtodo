//! The strict parser's error: which grammar rule failed and where.

use core::fmt;

/// A strict-mode parse failure. `rule` names the rule in `specs/todotxt.abnf` so an agent that sends
/// `(a) task` learns why (design §6.3); `byte` is the offset where matching stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// ABNF rule name, e.g. `"priority"`, `"date"`, `"completed"`.
    pub rule: &'static str,
    /// Byte offset into the line where the rule failed.
    pub byte: usize,
    /// One short sentence a human or agent can act on.
    pub message: &'static str,
}

impl ParseError {
    /// Builds an error; `byte` must be within or at the end of the line being parsed.
    pub const fn new(rule: &'static str, byte: usize, message: &'static str) -> ParseError {
        ParseError {
            rule,
            byte,
            message,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "byte {}: {} (rule `{}` in specs/todotxt.abnf)",
            self.byte, self.message, self.rule
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_rule_and_byte() {
        let e = ParseError::new(
            "priority",
            0,
            "priority must be an uppercase letter in parentheses",
        );
        assert_eq!(
            alloc::format!("{e}"),
            "byte 0: priority must be an uppercase letter in parentheses (rule `priority` in specs/todotxt.abnf)"
        );
    }
}
