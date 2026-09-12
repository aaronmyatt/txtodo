// PLACEHOLDER — the real module wraps the wasm-bindgen export built by a parallel task
// (`crates/txtodo-ffi/src/wasm.rs`, `parse_line_strict`, wasm32-unknown-unknown only; see that
// crate's CLAUDE.md) via a wasm-pack/vite-plugin-wasm loader and will overwrite this file wholesale
// on merge. Until then this stub matches the exact shape the popover consumes
// (tasks/desktop-edit-popover/notes.md) and always reports "ok" — strict-mode validation is
// advisory only (design §2.3: lenient save is always allowed), so a stubbed validator never blocks
// anything, it just means the inline error never lights up until the real wasm module lands.
// Ref: https://rustwasm.github.io/wasm-bindgen/

/** Mirrors `crates/txtodo-ffi/src/parse_check.rs`'s `StrictCheck`, reshaped for JS by `wasm.rs`. */
export type StrictParseResult =
	| { ok: true }
	| { ok: false; rule: string; byte: number; message: string };

/** Strict-mode check for one todo.txt line; see the module doc above for the stub's behavior. */
export async function parseLineStrict(raw: string): Promise<StrictParseResult> {
	void raw;
	return { ok: true };
}
