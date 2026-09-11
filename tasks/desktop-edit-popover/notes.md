# Edit popover: single-line CM6, token chips, strict validation via WASM core (plan M7, plan §3.2)

## Goal

Single click on a line opens a popover anchored below it: one single-line CM6 instance (same Lezer
language as the main view), pre-filled with the *raw* line including `id:`. Save goes through the
daemon's `Apply`. The UI never re-implements the grammar — strict validation is the WASM build of
`txtodo-core`, loaded into the Svelte app.

## Design

```ts
// apps/desktop/src/popover/Popover.svelte  (shared with quick-add per desktop-quick-add)
type Chip = "A" | "B" | "C" | "+" | "@" | "due:" | "t:" | "rec:" | "x";
function toggleComplete(raw: string, today: string): string;         // x <-> pri:<P> swap
function applyChip(raw: string, caret: number, chip: Chip): { text: string; caret: number };
async function save(raw: string, task: TaskRef): Promise<void>;      // Apply(Edit)
```

- **Token chips** (§3.2): `(A)` `(B)` `(C)` `+` `@` `due:` `t:` `rec:` `x`.
  - Priority chips *replace* the current priority, or remove it when the same chip is tapped again.
  - `x` toggles completion: on → prepend `x <today> ` and, if a priority exists, remove it and
    append `pri:<P>` (spec-conformant, per `core-complete-pri`); off → reverse that.
  - Other chips insert the token at the caret with a leading space.
- **Strict validation via WASM**: `txtodo-ffi` (wasm32 target) exports the strict parser over
  `wasm-bindgen` (https://rustwasm.github.io/wasm-bindgen/). `parse_line(raw, Mode::Strict)` returns
  a `Result<Line, ParseError>` whose lifetime cannot cross the boundary, so the ffi maps it to a
  JSON value first:

```rust
// crates/txtodo-ffi/src/wasm.rs  (wasm32-unknown-unknown target)
#[wasm_bindgen]
pub fn parse_line_strict(raw: &str) -> JsValue;   // { ok:true } | { ok:false, rule, byte, message }
#[wasm_bindgen]
pub fn diff_text(a: &str, b: &str) -> JsValue;    // char-level diff, reused by conflict review
```

  The popover calls `parse_line_strict` on each keystroke (debounced); `ParseError { rule, byte,
  message }` renders inline at 13 px in the danger colour. The user can still save in lenient mode
  with a quirk — never block saving text (design §2.3: lenient is read-only, quirks round-trip).
- **Footer**: `Line N · <device>, <relative time>` from the op log via `History` (the daemon's
  `HistoryRequest { file, task, limit }`), Cancel and Save. Enter saves, Esc cancels. A save that
  produces a line identical to the original is a no-op — no op-log entry.
- **Save path**: `Apply(Edit { task: TaskRef { line_number, id }, new_line })` — the two-way
  address lets the daemon reject a stale write against a moved/deleted line (the proto's `TaskRef`
  contract), rather than editing the wrong line.

## Placement/dependencies

- `apps/desktop/src/popover/` plus a new `crates/txtodo-ffi/src/wasm.rs` (wasm32 target, depends on
  `txtodo-core` only — `txtodo-ffi` already depends on core+query per plan §2). The wasm build is a
  new target; the wasm artifact is a generated binary, committed alone or built in CI, not mixed
  with source.
- Shares the Lezer language with `desktop-main-view`; shares the Svelte component with
  `desktop-quick-add` (extract `Popover.svelte` once, so quick-add cannot drift).

## Edge cases & invariants

- No-op save: compare the edited raw line to the original; identical → no `Apply`, no op-log entry.
- A chip insert must not double-space: insert the leading space only when the caret is not already
  at a word boundary.
- Lenient lines (`x` with no date, `(a)`, trailing whitespace) show the inline error but save; the
  quirk is recorded by the daemon's lenient parse, not by the UI.

## Acceptance

- Click opens the popover with the raw line including `id:`.
- Enter saves and only that line changes on disk (byte-diff before/after).
- A no-op save (unchanged text) emits no op-log entry.

## References

- plan M7 and §3.2 (txtodo-implementation-plan.md), design §2.3 and §7 (txtodo-design.md)
- wasm-bindgen: https://rustwasm.github.io/wasm-bindgen/ · CodeMirror 6: https://codemirror.net/
