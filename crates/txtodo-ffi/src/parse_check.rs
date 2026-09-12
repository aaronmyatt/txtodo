//! Pure logic behind the `parse_line_strict` wasm export (see [`crate::wasm`]). Kept free of
//! `wasm_bindgen`/`JsValue` so it can be unit-tested with plain `cargo test` on the host target;
//! the wasm module is a thin `JsValue`-shaping wrapper around [`check_strict`].

use txtodo_core::{Mode, parse_line};

/// The strict-mode parse outcome, shaped to mirror the JS union the popover consumes:
/// `{ ok: true }` or `{ ok: false, rule, byte, message }` (see `tasks/desktop-edit-popover/notes.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrictCheck {
    /// `true` when `raw` parses under [`Mode::Strict`] with no deviation.
    pub ok: bool,
    /// ABNF rule name that failed, or `None` when `ok`.
    pub rule: Option<&'static str>,
    /// Byte offset where the rule failed, or `None` when `ok`.
    pub byte: Option<usize>,
    /// One short, human/agent-actionable sentence, or `None` when `ok`.
    pub message: Option<&'static str>,
}

/// Runs `txtodo_core::parse_line(raw, Mode::Strict)` and reshapes the result for the popover's
/// inline error: still allows the caller to save in lenient mode (design §2.3) — this only
/// reports whether strict mode is happy, it never blocks anything itself.
pub fn check_strict(raw: &str) -> StrictCheck {
    match parse_line(raw, Mode::Strict) {
        Ok(_) => StrictCheck {
            ok: true,
            rule: None,
            byte: None,
            message: None,
        },
        Err(e) => StrictCheck {
            ok: false,
            rule: Some(e.rule),
            byte: Some(e.byte),
            message: Some(e.message),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_line_is_ok_with_no_error_fields() {
        let got = check_strict("(A) 2026-09-12 call mum +home @phone");
        assert_eq!(
            got,
            StrictCheck {
                ok: true,
                rule: None,
                byte: None,
                message: None
            }
        );
    }

    #[test]
    fn blank_line_is_ok() {
        assert!(check_strict("").ok);
    }

    #[test]
    fn lenient_only_quirk_fails_strict_with_rule_and_byte() {
        // Trailing whitespace is a lenient quirk (design §2.3), not valid strict-mode text: matches
        // txtodo-core's own `strict_err("task ")` case in parse_tests.rs.
        let got = check_strict("task ");
        assert_eq!(
            got,
            StrictCheck {
                ok: false,
                rule: Some("description"),
                byte: Some(4),
                message: Some("trailing whitespace"),
            }
        );
    }
}
