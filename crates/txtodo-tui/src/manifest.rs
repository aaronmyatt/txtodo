//! `specs/client-parity.toml`, built into the binary (ADR 0031, task `tui-revamp/tui-settings`):
//! the Help screen and Settings › Shortcuts render from it, and `tests/parity.rs` holds the keymap
//! to it. Read with a small parser for the subset the manifest uses (one `key = value` per line;
//! values are basic strings, arrays of strings, or inline tables of those), the same one the
//! desktop's `keys.parity.test.ts` carries, so this crate takes no TOML dependency. Anything outside
//! that subset fails the parse loudly, which the parity test would catch before a release.
//! Ref: <https://toml.io/en/v1.0.0> (basic strings, arrays, inline tables)

use std::collections::BTreeMap;

/// The manifest's text, as built.
pub const MANIFEST: &str = include_str!("../../../specs/client-parity.toml");

/// A value in the manifest's subset. Tables hold strings and arrays only.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A basic string.
    Str(String),
    /// An array of strings.
    List(Vec<String>),
    /// An inline table of those.
    Table(BTreeMap<String, Value>),
}

/// One `[[action]]` or `[[screen]]` row: its keys and values.
pub type Row = BTreeMap<String, Value>;

/// A basic string at the start of `text`: its value and the rest of `text`.
fn take_string(text: &str) -> (String, &str) {
    let body = text
        .strip_prefix('"')
        .unwrap_or_else(|| panic!("expected a string at: {text}"));
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return (out, body[i + 1..].trim_start()),
            '\\' => out.extend(chars.next().map(|(_, e)| e)),
            _ => out.push(c),
        }
    }
    panic!("unterminated string: {text}")
}

/// Drops one `,` separator, if there is one.
fn skip_comma(text: &str) -> &str {
    text.strip_prefix(',').map_or(text, str::trim_start)
}

/// A string, an array of strings, or an inline table of those, at the start of `text`.
fn take_value(text: &str) -> (Value, &str) {
    if text.starts_with('"') {
        let (s, rest) = take_string(text);
        return (Value::Str(s), rest);
    }
    if let Some(mut rest) = text.strip_prefix('[').map(str::trim_start) {
        let mut out = Vec::new();
        while !rest.starts_with(']') {
            let (s, after) = take_string(rest);
            out.push(s);
            rest = skip_comma(after);
        }
        return (Value::List(out), rest[1..].trim_start());
    }
    if let Some(mut rest) = text.strip_prefix('{').map(str::trim_start) {
        let mut table = BTreeMap::new();
        while !rest.starts_with('}') {
            let (key, after) = rest
                .split_once('=')
                .unwrap_or_else(|| panic!("expected a key at: {rest}"));
            let (value, after) = take_value(after.trim_start());
            assert!(!matches!(value, Value::Table(_)), "nested table at: {rest}");
            table.insert(key.trim().to_owned(), value);
            rest = skip_comma(after);
        }
        return (Value::Table(table), rest[1..].trim_start());
    }
    panic!("unsupported value: {text}")
}

/// Every `[[action]]` row, in file order (`[[screen]]` rows are parsed, then dropped).
pub fn actions() -> Vec<Row> {
    let mut out: Vec<(bool, Row)> = Vec::new();
    for raw in MANIFEST.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "[[action]]" => out.push((true, Row::new())),
            "[[screen]]" => out.push((false, Row::new())),
            _ => {
                let (key, value) = line
                    .split_once('=')
                    .unwrap_or_else(|| panic!("unexpected line: {raw}"));
                let (value, rest) = take_value(value.trim_start());
                assert!(rest.is_empty(), "trailing text: {raw}");
                let (_, row) = out
                    .last_mut()
                    .unwrap_or_else(|| panic!("a key before any row: {raw}"));
                row.insert(key.trim().to_owned(), value);
            }
        }
    }
    out.into_iter()
        .filter_map(|(is_action, row)| is_action.then_some(row))
        .collect()
}

/// One action as Help and Shortcuts show it: its title, its TUI keys and where they work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// The manifest id, also the `:` command.
    pub id: String,
    /// What it does.
    pub title: String,
    /// The TUI's keys (its own, else the shared ones); empty for click or `:` only.
    pub keys: Vec<String>,
    /// Where they work.
    pub scope: String,
}

/// Every action the TUI has built (`tui.status` done or differs), in manifest order.
pub fn shortcuts() -> Vec<Shortcut> {
    actions()
        .iter()
        .filter_map(|row| {
            let Some(Value::Table(tui)) = row.get("tui") else {
                return None;
            };
            let built = matches!(
                tui.get("status"),
                Some(Value::Str(s)) if s == "done" || s == "differs"
            );
            let text = |key: &str| match row.get(key) {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            };
            let keys = match (tui.get("keys"), row.get("keys")) {
                (Some(Value::List(k)), _) | (None, Some(Value::List(k))) => k.clone(),
                _ => Vec::new(),
            };
            built.then(|| Shortcut {
                id: text("id"),
                title: text("title"),
                keys,
                scope: text("scope"),
            })
        })
        .collect()
}

/// Settings › Shortcuts: one row per built action, its keys (or `:id`) as the value.
pub fn shortcut_rows() -> Vec<crate::settings_rows::SRow> {
    shortcuts()
        .into_iter()
        .map(|s| {
            let keys = if s.keys.is_empty() {
                format!(":{}", s.id)
            } else {
                s.keys.join(" / ")
            };
            crate::settings_rows::SRow {
                label: s.title,
                value: format!("{keys} \u{b7} {}", s.scope),
                ..crate::settings_rows::SRow::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_actions_come_with_their_tui_keys() {
        let all = shortcuts();
        let quit = all.iter().find(|s| s.id == "app.quit");
        assert_eq!(
            quit.map(|s| s.keys.clone()),
            Some(vec!["Ctrl-c".to_owned()])
        );
        assert!(all.iter().all(|s| !s.title.is_empty()));
        assert!(
            !all.iter().any(|s| s.id == "app.pin"),
            "na rows are not shortcuts"
        );
    }
}
