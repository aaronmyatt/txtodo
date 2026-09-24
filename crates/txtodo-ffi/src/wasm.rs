//! wasm-bindgen exports over the pure logic in [`crate::parse_check`], [`crate::diff_view`] and,
//! for the logic every client shares (task `tui-revamp/shared-core`), [`crate::shared`] over
//! `txtodo-core`'s `query`, `strict_hint`, `chips` and `universal`.
//! Compiled only for `wasm32-unknown-unknown` (see this crate's `Cargo.toml` target-specific
//! `wasm-bindgen`/`js-sys` dependencies) — every export here is a thin `JsValue`-shaping wrapper;
//! see those modules for the tested logic.
//! Ref: https://rustwasm.github.io/wasm-bindgen/ and
//! https://docs.rs/wasm-bindgen/0.2/wasm_bindgen/struct.JsValue.html

use crate::diff_view::{DiffOp, DiffSegment, diff_segments};
use crate::parse_check::{StrictCheck, check_strict};
use crate::shared::{OwnedRow, apply_chip_utf16, group_owned, parse_group_by};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

/// Strict-mode validity check for one `todo.txt` line, for the edit popover's inline error
/// (`tasks/desktop-edit-popover/notes.md`). Returns `{ ok: true }` when `raw` parses cleanly under
/// strict mode, or `{ ok: false, rule: string, byte: number, message: string }` otherwise — it
/// never blocks saving, the popover still allows a lenient-mode save with the quirk (design §2.3).
#[wasm_bindgen]
pub fn parse_line_strict(raw: &str) -> JsValue {
    strict_check_to_js(check_strict(raw))
}

/// Char-level diff between `a` ("mine") and `b` ("theirs"), for the conflict-review `DiffView`
/// (`tasks/desktop-conflict-review/notes.md`). Returns a JS array of `{ op: "equal" | "insert" |
/// "delete", text: string }` segments, in order; concatenating every segment's `text` reconstructs `b`.
#[wasm_bindgen]
pub fn diff_text(a: &str, b: &str) -> JsValue {
    let out = Array::new();
    for seg in diff_segments(a, b) {
        out.push(&segment_to_js(&seg));
    }
    out.into()
}

/// Whether `raw` matches every term of `query` (`txtodo_core::query`): the search every client
/// shares.
#[wasm_bindgen]
pub fn matches_query(raw: &str, query: &str) -> bool {
    txtodo_core::query::matches(raw, query)
}

/// The prompt bar's strict-mode hint for `line`, or `null` (`txtodo_core::strict_hint`).
#[wasm_bindgen]
pub fn strict_hint(line: &str) -> Option<String> {
    txtodo_core::strict_hint::strict_hint(line).map(String::from)
}

/// One chip tap: `{ text, caret }`, the caret in UTF-16 units both ways; `null` for an unknown
/// chip. Chip names: `A` `B` `C` `x` `+` `@` `due:` `t:` `rec:` (`txtodo_core::chips`).
#[wasm_bindgen]
pub fn apply_chip(raw: &str, caret: u32, chip: &str, today: &str) -> JsValue {
    let Some((text, caret)) = apply_chip_utf16(raw, caret as usize, chip, today) else {
        return JsValue::NULL;
    };
    let obj = Object::new();
    set(&obj, "text", JsValue::from_str(&text));
    set(&obj, "caret", JsValue::from_f64(caret as f64));
    obj.into()
}

/// Completes or reopens `raw` (`txtodo_core::chips::toggle_complete_text`).
#[wasm_bindgen]
pub fn toggle_complete_text(raw: &str, today: &str) -> String {
    txtodo_core::chips::toggle_complete_text(raw, today)
}

/// The due bucket's heading for `due` against `today`: `Overdue` `Today` `This week` `Later`
/// `No date` (`txtodo_core::universal::due_bucket`).
#[wasm_bindgen]
pub fn due_bucket(due: Option<String>, today: &str) -> String {
    let bucket = txtodo_core::universal::due_bucket(due.as_deref(), today);
    String::from(bucket.label())
}

/// A row's due badge `{ text, days }`, or `null` for no date (`txtodo_core::universal::due_label`).
#[wasm_bindgen]
pub fn due_label(due: Option<String>, today: &str) -> JsValue {
    let Some((text, days)) = txtodo_core::universal::due_label(due.as_deref(), today) else {
        return JsValue::NULL;
    };
    let obj = Object::new();
    set(&obj, "text", JsValue::from_str(&text));
    set(&obj, "days", JsValue::from_f64(days as f64));
    obj.into()
}

/// Groups Universal rows: `rows` is an array of `{ done, priority?, due?, project?, context?,
/// workspace }`, `by` one of `priority` `due` `project` `context` `workspace`, `workspaces` the
/// workspace order. Returns `[{ name, rows: number[] }]` in display order, or `null` for an
/// unknown `by` (`txtodo_core::universal::group`).
#[wasm_bindgen]
pub fn group_rows(rows: Array, by: &str, today: &str, workspaces: Array) -> JsValue {
    let Some(by) = parse_group_by(by) else {
        return JsValue::NULL;
    };
    let rows: Vec<OwnedRow> = rows.iter().map(|r| row_from_js(&r)).collect();
    let order: Vec<String> = workspaces.iter().filter_map(|w| w.as_string()).collect();
    let out = Array::new();
    for (name, members) in group_owned(&rows, by, today, &order) {
        let obj = Object::new();
        set(&obj, "name", JsValue::from_str(&name));
        let idx = Array::new();
        for i in members {
            idx.push(&JsValue::from_f64(i as f64));
        }
        set(&obj, "rows", idx.into());
        out.push(&obj);
    }
    out.into()
}

/// One `{ done, priority?, due?, project?, context?, workspace }` object; a missing or mistyped
/// field reads as absent.
fn row_from_js(value: &JsValue) -> OwnedRow {
    let field = |key: &str| Reflect::get(value, &JsValue::from_str(key)).ok();
    let text = |key: &str| field(key).and_then(|v| v.as_string());
    OwnedRow {
        done: field("done").and_then(|v| v.as_bool()).unwrap_or(false),
        priority: text("priority").and_then(|p| p.chars().next()),
        due: text("due"),
        project: text("project"),
        context: text("context"),
        workspace: text("workspace").unwrap_or_default(),
    }
}

/// Builds the `{ ok, rule?, byte?, message? }` object described on [`parse_line_strict`].
fn strict_check_to_js(check: StrictCheck) -> JsValue {
    let obj = Object::new();
    set(&obj, "ok", JsValue::from_bool(check.ok));
    if let Some(rule) = check.rule {
        set(&obj, "rule", JsValue::from_str(rule));
    }
    if let Some(byte) = check.byte {
        set(&obj, "byte", JsValue::from_f64(byte as f64));
    }
    if let Some(message) = check.message {
        set(&obj, "message", JsValue::from_str(message));
    }
    obj.into()
}

/// Builds one `{ op, text }` segment object described on [`diff_text`].
fn segment_to_js(seg: &DiffSegment) -> JsValue {
    let obj = Object::new();
    set(&obj, "op", JsValue::from_str(op_name(seg.op)));
    set(&obj, "text", JsValue::from_str(&seg.text));
    obj.into()
}

/// The JS-side string for a [`DiffOp`] variant.
fn op_name(op: DiffOp) -> &'static str {
    match op {
        DiffOp::Equal => "equal",
        DiffOp::Insert => "insert",
        DiffOp::Delete => "delete",
    }
}

/// `Reflect::set` on a plain object we just created: per MDN, `Reflect.set` only fails for
/// non-writable/non-configurable targets, which a fresh `Object` never is, so the `Result` is
/// ignored rather than unwrapped.
/// Ref: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Reflect/set
fn set(obj: &Object, key: &str, value: JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), &value);
}
