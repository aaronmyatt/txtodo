/* tslint:disable */
/* eslint-disable */

/**
 * Char-level diff between `a` ("mine") and `b` ("theirs"), for the conflict-review `DiffView`
 * (`tasks/desktop-conflict-review/notes.md`). Returns a JS array of `{ op: "equal" | "insert" |
 * "delete", text: string }` segments, in order; concatenating every segment's `text` reconstructs `b`.
 */
export function diff_text(a: string, b: string): any;

/**
 * Strict-mode validity check for one `todo.txt` line, for the edit popover's inline error
 * (`tasks/desktop-edit-popover/notes.md`). Returns `{ ok: true }` when `raw` parses cleanly under
 * strict mode, or `{ ok: false, rule: string, byte: number, message: string }` otherwise — it
 * never blocks saving, the popover still allows a lenient-mode save with the quirk (design §2.3).
 */
export function parse_line_strict(raw: string): any;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly diff_text: (a: number, b: number, c: number, d: number) => any;
    readonly parse_line_strict: (a: number, b: number) => any;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
