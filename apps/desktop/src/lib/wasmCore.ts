// Thin, typed wrapper over the txtodo-ffi WASM module (crates/txtodo-ffi/src/wasm.rs).
// Shared by the line editor (strict hint, chips), the search, the fields view and the
// conflict-review DiffView so none reinvents wasm loading. The Rust calls are synchronous; the
// `async` here only hides the one-time WASM `init()` from callers.
import init, { apply_chip, breakdown_fields, diff_text, strict_hint } from "./wasm-core/txtodo_ffi";

/** One segment of a char-level diff; concatenating every segment's `text` reconstructs `b`. */
export interface DiffSegment {
  op: "equal" | "insert" | "delete";
  text: string;
}

let ready: Promise<void> | null = null;
let loaded = false;

function ensureInit(): Promise<void> {
  ready ??= init().then(() => {
    loaded = true;
  });
  return ready;
}

/** Resolves once the wasm core is loaded; after that its exports may be called synchronously
 * (the editor's search needs that). */
export function wasmReady(): Promise<void> {
  return ensureInit();
}

/** Whether the wasm core is loaded (so a synchronous caller can use it right now). */
export function isWasmLoaded(): boolean {
  return loaded;
}

/** For tests: marks the core loaded after the caller ran `initSync` itself. */
export function markWasmLoaded(): void {
  loaded = true;
}

/** Char-level diff between `a` ("mine") and `b` ("theirs") for the conflict-review DiffView. */
export async function diffText(a: string, b: string): Promise<DiffSegment[]> {
  await ensureInit();
  return diff_text(a, b) as DiffSegment[];
}

/** One row of a line shown field by field (`txtodo_core::breakdown::fields`). */
export interface BreakdownField {
  name: string;
  value: string;
}

/** `line` field by field, like sqlite's `.mode line`: the rows the TUI's `v` shows. */
export async function breakdownFields(line: string): Promise<BreakdownField[]> {
  await ensureInit();
  return breakdown_fields(line) as BreakdownField[];
}

/** The prompt bar's one-line strict-mode hint for `line` (lower-case priority, a leading `(` that is
 * not a priority, a `due:`/`t:` without a date), or null. The shared core's words
 * (`txtodo_core::strict_hint`), the same the TUI shows. Never blocks a save. */
export async function strictHint(line: string): Promise<string | null> {
  await ensureInit();
  return strict_hint(line) ?? null;
}

/** One chip tap through the shared core (`txtodo_core::chips`, the TUI's prompt bar's rules):
 * the new text and caret, carets in UTF-16 units like CodeMirror's; null for an unknown chip. */
export async function applyChipCore(
  raw: string,
  caret: number,
  chip: string,
  today: string
): Promise<{ text: string; caret: number } | null> {
  await ensureInit();
  return (apply_chip(raw, caret, chip, today) as { text: string; caret: number } | null) ?? null;
}
