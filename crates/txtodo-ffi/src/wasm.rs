//! wasm-bindgen exports over the pure logic in [`crate::parse_check`] and [`crate::diff_view`].
//! Compiled only for `wasm32-unknown-unknown` (see this crate's `Cargo.toml` target-specific
//! `wasm-bindgen`/`js-sys` dependencies) — every export here is a thin `JsValue`-shaping wrapper;
//! see those modules for the tested logic.
//! Ref: https://rustwasm.github.io/wasm-bindgen/ and
//! https://docs.rs/wasm-bindgen/0.2/wasm_bindgen/struct.JsValue.html

use crate::diff_view::{DiffOp, DiffSegment, diff_segments};
use crate::parse_check::{StrictCheck, check_strict};
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
