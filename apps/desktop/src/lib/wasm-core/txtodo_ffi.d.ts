/* tslint:disable */
/* eslint-disable */

/**
 * One chip tap: `{ text, caret }`, the caret in UTF-16 units both ways; `null` for an unknown
 * chip. Chip names: `A` `B` `C` `x` `+` `@` `due:` `t:` `rec:` (`txtodo_core::chips`).
 */
export function apply_chip(raw: string, caret: number, chip: string, today: string): any;

/**
 * Char-level diff between `a` ("mine") and `b` ("theirs"), for the conflict-review `DiffView`
 * (`tasks/desktop-conflict-review/notes.md`). Returns a JS array of `{ op: "equal" | "insert" |
 * "delete", text: string }` segments, in order; concatenating every segment's `text` reconstructs `b`.
 */
export function diff_text(a: string, b: string): any;

/**
 * The due bucket's heading for `due` against `today`: `Overdue` `Today` `This week` `Later`
 * `No date` (`txtodo_core::universal::due_bucket`).
 */
export function due_bucket(due: string | null | undefined, today: string): string;

/**
 * A row's due badge `{ text, days }`, or `null` for no date (`txtodo_core::universal::due_label`).
 */
export function due_label(due: string | null | undefined, today: string): any;

/**
 * Groups Universal rows: `rows` is an array of `{ done, priority?, due?, project?, context?,
 * workspace }`, `by` one of `priority` `due` `project` `context` `workspace`, `workspaces` the
 * workspace order. Returns `[{ name, rows: number[] }]` in display order, or `null` for an
 * unknown `by` (`txtodo_core::universal::group`).
 */
export function group_rows(rows: Array<any>, by: string, today: string, workspaces: Array<any>): any;

/**
 * Whether `raw` matches every term of `query` (`txtodo_core::query`): the search every client
 * shares.
 */
export function matches_query(raw: string, query: string): boolean;

/**
 * Strict-mode validity check for one `todo.txt` line, for the edit popover's inline error
 * (`tasks/desktop-edit-popover/notes.md`). Returns `{ ok: true }` when `raw` parses cleanly under
 * strict mode, or `{ ok: false, rule: string, byte: number, message: string }` otherwise — it
 * never blocks saving, the popover still allows a lenient-mode save with the quirk (design §2.3).
 */
export function parse_line_strict(raw: string): any;

/**
 * The prompt bar's strict-mode hint for `line`, or `null` (`txtodo_core::strict_hint`).
 */
export function strict_hint(line: string): string | undefined;

/**
 * Completes or reopens `raw` (`txtodo_core::chips::toggle_complete_text`).
 */
export function toggle_complete_text(raw: string, today: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly apply_chip: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => any;
    readonly diff_text: (a: number, b: number, c: number, d: number) => any;
    readonly due_bucket: (a: number, b: number, c: number, d: number) => [number, number];
    readonly due_label: (a: number, b: number, c: number, d: number) => any;
    readonly group_rows: (a: any, b: number, c: number, d: number, e: number, f: any) => any;
    readonly matches_query: (a: number, b: number, c: number, d: number) => number;
    readonly parse_line_strict: (a: number, b: number) => any;
    readonly strict_hint: (a: number, b: number) => [number, number];
    readonly toggle_complete_text: (a: number, b: number, c: number, d: number) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
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
