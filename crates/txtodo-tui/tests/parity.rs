//! `specs/client-parity.toml`, checked from the TUI side (ADR 0031, task
//! `tasks/tui-revamp/parity-manifest`): every row the manifest marks built in the TUI is bound in
//! `keymap::BINDINGS` with the same keys and scope, every binding has such a row, and every
//! `differs`/`na` row says why. `apps/desktop/src/lib/keys.parity.test.ts` is the desktop twin.
//!
//! The manifest is read with a small parser for the subset it uses (one `key = value` per line;
//! values are basic strings, arrays of strings, or inline tables of those), the same one the
//! desktop test carries, so this crate takes no TOML dependency for a test. Anything outside that
//! subset fails the parse loudly.
//! Ref: <https://toml.io/en/v1.0.0> (basic strings, arrays, inline tables)

use std::collections::BTreeMap;

use txtodo_tui::keymap::{BINDINGS, Binding};

const MANIFEST: &str = include_str!("../../../specs/client-parity.toml");

/// A value in the manifest's subset. Tables hold strings and arrays only.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Str(String),
    List(Vec<String>),
    Table(BTreeMap<String, Value>),
}

type Row = BTreeMap<String, Value>;

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
fn actions() -> Vec<Row> {
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

fn text<'a>(row: &'a Row, key: &str) -> &'a str {
    match row.get(key) {
        Some(Value::Str(s)) => s,
        other => panic!("{key} is not a string: {other:?}"),
    }
}

fn list(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::List(keys)) => Some(keys.clone()),
        None => None,
        other => panic!("not a list: {other:?}"),
    }
}

/// A client's `{ status, surface, keys? }`.
fn client<'a>(row: &'a Row, name: &str) -> &'a BTreeMap<String, Value> {
    match row.get(name) {
        Some(Value::Table(t)) => t,
        other => panic!("{}: {name} is not a table: {other:?}", text(row, "id")),
    }
}

fn status(row: &Row, name: &str) -> String {
    match client(row, name).get("status") {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("{}: {name}.status: {other:?}", text(row, "id")),
    }
}

/// The keys the TUI should bind for `row`: its own `tui.keys` when it has them, else the shared.
fn tui_keys(row: &Row) -> Vec<String> {
    list(client(row, "tui").get("keys"))
        .or_else(|| list(row.get("keys")))
        .unwrap_or_default()
}

fn binding(id: &str) -> Option<&'static Binding> {
    BINDINGS.iter().find(|b| b.command.id() == id)
}

fn sorted(keys: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut keys: Vec<String> = keys.into_iter().collect();
    keys.sort();
    keys
}

#[test]
fn every_row_built_in_the_tui_is_bound_with_its_keys_and_scope() {
    let mut checked = 0;
    for row in actions() {
        let id = text(&row, "id");
        let built = status(&row, "tui");
        let Some(b) = binding(id) else {
            // A click-only action (no key anywhere, like a breadcrumb or a drag) has nothing to
            // bind; any other done row must be in the keymap.
            assert!(
                built != "done" || tui_keys(&row).is_empty(),
                "{id} is done in the manifest but not in keymap::BINDINGS"
            );
            continue;
        };
        assert!(
            built == "done" || built == "differs",
            "{id} is bound but the manifest says tui.status = {built}"
        );
        assert_eq!(b.scope.name(), text(&row, "scope"), "{id}: scope");
        let bound = sorted(b.keys.iter().map(|k| (*k).to_owned()));
        assert_eq!(bound, sorted(tui_keys(&row)), "{id}: keys");
        checked += 1;
    }
    assert_eq!(checked, BINDINGS.len(), "every binding has a manifest row");
}

#[test]
fn every_id_is_unique_and_every_deviation_is_written_down() {
    let rows = actions();
    let mut seen = std::collections::BTreeSet::new();
    for row in &rows {
        let id = text(row, "id");
        assert!(seen.insert(id), "{id} twice");
        let deviates = ["desktop", "tui"]
            .iter()
            .any(|c| matches!(status(row, c).as_str(), "differs" | "na"));
        let deviation = row.get("deviation").map_or("", |_| text(row, "deviation"));
        // One way only: a planned row may explain its keys ahead of time (`prompt.focus`).
        assert!(
            !deviates || !deviation.is_empty(),
            "{id}: a differs/na row needs a deviation"
        );
    }
    assert!(rows.len() > BINDINGS.len());
}
