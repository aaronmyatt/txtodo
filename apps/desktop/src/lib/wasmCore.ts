// Thin, typed wrapper over the txtodo-ffi WASM module (crates/txtodo-ffi/src/wasm.rs).
// Shared by the edit popover (strict-mode validation) and the conflict-review DiffView so
// neither reinvents wasm loading. Both underlying calls are synchronous in Rust; the `async`
// here is only to hide the one-time WASM `init()` from callers.
import init, { diff_text, parse_line_strict } from "./wasm-core/txtodo_ffi";

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

function ensureInit(): Promise<void> {
  ready ??= init().then(() => undefined);
  return ready;
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
