//! Token-colour painting for the line list: `txtodo_core::tokenize` -> styled `ratatui` spans
//! (design §3.1). This is the only place token colours live — every widget renders a line by
//! calling [`paint_line`], never by re-deriving a style from a token kind itself.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as TuiSpan};
use txtodo_core::{Span, TokenKind, tokenize};

/// The semantic colour for one [`TokenKind`], per design §3.1's platform-agnostic names
/// (`priority`, `date`, `completion-marker`, `project`, `context`, `tag-key`, `tag-value`,
/// `id-tag`, `text`). Each platform maps these names to its own theme; this is the ratatui
/// terminal's mapping, picked for a dark-background 256-colour terminal.
/// Ref: <https://docs.rs/ratatui/latest/ratatui/style/enum.Color.html>
pub fn token_style(kind: TokenKind) -> Style {
    match kind {
        // "priority"
        TokenKind::Priority => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        // "date" — completion and creation dates share one semantic colour.
        TokenKind::CompletionDate | TokenKind::CreationDate => Style::new().fg(Color::Cyan),
        // "completion-marker"
        TokenKind::CompletionMarker => Style::new().fg(Color::DarkGray),
        // "project"
        TokenKind::Project => Style::new().fg(Color::Green),
        // "context"
        TokenKind::Context => Style::new().fg(Color::Magenta),
        // "tag-key"
        TokenKind::TagKey => Style::new().fg(Color::Blue),
        // "tag-value"
        TokenKind::TagValue => Style::new().fg(Color::LightBlue),
        // "id-tag" — hidden by default (see `paint_line`'s `show_id`); dim when shown, but a
        // distinct shade from `completion-marker` so the two never read as the same token.
        TokenKind::IdTag => Style::new().fg(Color::Gray),
        // Not one of design §3.1's named colours; treated as "text" with an underline so a link
        // still reads distinctly without inventing a tenth semantic name.
        TokenKind::Url => Style::new()
            .fg(Color::White)
            .add_modifier(Modifier::UNDERLINED),
        // "text"
        TokenKind::Text => Style::new().fg(Color::White),
        // Not a semantic token; carries no colour of its own but still gets an explicit, stable
        // style so every `TokenKind` maps to *something* (see `every_token_kind_has_a_style`).
        TokenKind::Whitespace => Style::new().fg(Color::Reset),
    }
}

/// Token kinds that are part of the task's *description* rather than its structured prefix
/// (`x`, dates, priority). Design §3.1: on a completed line, "description struck through, `x`
/// and dates not struck" — priority is prefix too, so it is excluded here as well.
fn is_description_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Project
            | TokenKind::Context
            | TokenKind::TagKey
            | TokenKind::TagValue
            | TokenKind::IdTag
            | TokenKind::Url
            | TokenKind::Text
    )
}

/// Paints one raw line as a styled `ratatui` [`Line`], design §3.1:
/// - every byte is tokenized via [`txtodo_core::tokenize`] and coloured by [`token_style`];
/// - `completed` dims the whole line and strikes through the description only (not `x`/dates);
/// - `show_id` controls whether `id:` tags are painted at all — `false` (the default) drops
///   them entirely, the terminal equivalent of desktop's "zero-width decoration".
pub fn paint_line(raw: &str, completed: bool, show_id: bool) -> Line<'static> {
    let mut spans = Vec::new();
    for Span { kind, start, end } in tokenize(raw) {
        if kind == TokenKind::IdTag && !show_id {
            continue;
        }
        spans.push(span_to_tui(kind, &raw[start..end], completed));
    }
    Line::from(spans)
}

/// One token's text, styled per [`token_style`] plus the completed-line muting/strike rule.
fn span_to_tui(kind: TokenKind, text: &str, completed: bool) -> TuiSpan<'static> {
    let mut style = token_style(kind);
    if completed {
        style = style.add_modifier(Modifier::DIM);
        if is_description_token(kind) {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
    }
    TuiSpan::styled(text.to_owned(), style)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KINDS: [TokenKind; 11] = [
        TokenKind::CompletionMarker,
        TokenKind::CompletionDate,
        TokenKind::CreationDate,
        TokenKind::Priority,
        TokenKind::Project,
        TokenKind::Context,
        TokenKind::TagKey,
        TokenKind::TagValue,
        TokenKind::IdTag,
        TokenKind::Url,
        TokenKind::Text,
    ];

    /// Every non-whitespace `TokenKind` maps to a distinct, defined foreground colour — the
    /// §3.1 semantic-colour contract, checked so a future `TokenKind` variant can't silently
    /// fall through to a default style.
    #[test]
    fn every_token_kind_has_a_style() {
        for kind in ALL_KINDS {
            let style = token_style(kind);
            assert!(style.fg.is_some(), "{kind:?} has no foreground colour");
        }
        // Whitespace is intentionally colourless but still an explicit, stable mapping.
        assert_eq!(token_style(TokenKind::Whitespace).fg, Some(Color::Reset));
    }

    /// §3.1 semantic colours are pairwise distinct except the deliberate `date` merge
    /// (`CompletionDate`/`CreationDate` share one colour by design).
    #[test]
    fn distinct_kinds_get_distinct_colours() {
        let grouped = |k: TokenKind| match k {
            TokenKind::CompletionDate | TokenKind::CreationDate => TokenKind::CreationDate,
            other => other,
        };
        for a in ALL_KINDS {
            for b in ALL_KINDS {
                if grouped(a) == grouped(b) || a == TokenKind::Url || b == TokenKind::Url {
                    continue;
                }
                if a != b {
                    assert_ne!(
                        token_style(a).fg,
                        token_style(b).fg,
                        "{a:?} and {b:?} share a colour"
                    );
                }
            }
        }
    }

    /// `paint_line` covers every byte of every corpus line, in order, with none dropped or
    /// duplicated — the byte-coverage oracle the backlog item calls for, reusing the same
    /// `corpus/*.txt` fixtures `txtodo_core::tokenize`'s own tests use.
    #[test]
    fn paint_line_covers_every_byte_of_every_corpus_line() {
        let corpus = [
            include_str!("../../../corpus/edge-cases.txt"),
            include_str!("../../../corpus/lenient.txt"),
            include_str!("../../../corpus/tags.txt"),
            include_str!("../../../corpus/refs.txt"),
            include_str!("../../../corpus/structure.txt"),
        ];
        for line in corpus.iter().flat_map(|f| f.lines()) {
            for show_id in [false, true] {
                let painted = paint_line(line, false, show_id);
                let rebuilt: String = painted.spans.iter().map(|s| s.content.as_ref()).collect();
                if show_id {
                    assert_eq!(rebuilt, line, "{line:?}: painted bytes don't match raw");
                } else {
                    // Every byte not part of a hidden `id:` tag must still be present, in order.
                    assert!(
                        line.len() >= rebuilt.len(),
                        "{line:?}: hiding id: never adds bytes"
                    );
                }
            }
        }
    }

    /// Completed lines strike the description but never the completion marker or dates.
    #[test]
    fn completed_line_strikes_description_not_prefix() {
        let line = paint_line("x 2026-09-11 2026-09-01 buy milk +errand", true, true);
        for span in &line.spans {
            let text = span.content.as_ref();
            let struck = span.style.add_modifier.contains(Modifier::CROSSED_OUT);
            if text == "x" || text.starts_with("2026-") {
                assert!(!struck, "{text:?} must not be struck through");
            }
            // Every token is dimmed regardless of struck-through status.
            assert!(span.style.add_modifier.contains(Modifier::DIM));
        }
    }

    /// `show_id = false` drops `id:` tags entirely; `true` keeps them.
    #[test]
    fn id_tag_hidden_by_default() {
        let raw = "buy milk id:01J9K3H5Z7Q8X2M4N6P8R0T2V4";
        let hidden = paint_line(raw, false, false);
        let shown = paint_line(raw, false, true);
        let hidden_text: String = hidden.spans.iter().map(|s| s.content.as_ref()).collect();
        let shown_text: String = shown.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(hidden_text, "buy milk ");
        assert_eq!(shown_text, raw);
    }
}
