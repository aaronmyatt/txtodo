// Thin, typed wrapper over the txtodo-ffi WASM module (crates/txtodo-ffi/src/wasm.rs).
// Shared by the edit popover (strict-mode validation) and the conflict-review DiffView so
// neither reinvents wasm loading. Both underlying calls are synchronous in Rust; the `async`
// here is only to hide the one-time WASM `init()` from callers.
import init, { breakdown_fields, diff_text, parse_line_strict } from "./wasm-core/txtodo_ffi";

/** Mirrors `crates/txtodo-ffi/src/parse_check.rs`'s `StrictCheck`. */
export type StrictCheckResult =
  | { ok: true }
  | { ok: false; rule: string; byte: number; message: string };

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

/** Strict-mode validity check for one todo.txt line (edit popover's inline error). */
export async function parseLineStrict(raw: string): Promise<StrictCheckResult> {
  await ensureInit();
  return parse_line_strict(raw) as StrictCheckResult;
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
