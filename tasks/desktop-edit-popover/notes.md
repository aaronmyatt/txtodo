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

## As built (2026-09-13, agent)

Found already substantially built from an earlier session — and the WASM dependency this task
flags as "investigate before assuming" turned out to be **done**: `crates/txtodo-ffi/src/wasm.rs`
exports `parse_line_strict`/`diff_text` over `wasm-bindgen` (target `wasm32-unknown-unknown`), the
compiled artifact is checked in at `apps/desktop/src/lib/wasm-core/txtodo_ffi_bg.wasm` (55 KB) with
its `.js`/`.d.ts` glue, `apps/desktop/scripts/build-wasm-core.sh` regenerates it, and
`$lib/wasmCore.ts` wraps both calls with the one-time `init()` hidden behind two typed async
functions. `EditPopover.svelte` already had: the raw line (id: included) in a single-line CM6
instance, all nine token chips (`editPopoverLogic.ts`'s `applyChip`/`toggleComplete`, unit-tested in
`__tests__/editPopoverLogic.test.ts`), debounced strict-mode validation via `parseLineStrict`, a
`History`-sourced footer, and Enter-saves/Esc-cancels via a `Prec.highest` keymap.

**Bug found and fixed this session**: `EditPopover`'s `saveAndClose` called `applyEdit` (the Tauri
`apply` command) itself, *and* every host (`MainView.svelte`'s `savePopover`, `FileView.svelte`'s
`saveLocalEdit`) called `applyMutations` again in its own `onSave` callback — every edit through
the *hosted* path (the one `MainView` actually uses) was applied **twice**, appending two identical
`edit_text` ops per save. Fixed by inverting control: `EditPopover` now only calls the `onSave`
prop and never touches `$lib/daemon` itself (see the module doc added to
`apps/desktop/src/lib/components/EditPopover.svelte`); `MainView`/`FileView`'s existing `onSave`
callbacks were already the correct single point of the real `Apply` call, so they needed no change.
This also directly enabled reuse for `desktop-quick-add`: `taskRef` widened to `TaskRef | null`
(`null` = no existing line yet, no `Line N` footer, no `History` lookup) and `onSave` widened to
`(text: string) => void | Promise<void>`, so the same component now serves the main view/detail
view (host performs an `Edit`) and quick-add (host performs an `Add`) without forking into two
components — see `desktop-quick-add`'s "As built" for why `EditPopover.svelte` itself (not a new
`Popover.svelte`) is that shared component.

Also added an optional `onDirtyChange?: (dirty: boolean) => void` prop (fires on every keystroke,
comparing against `initialLine`) for `desktop-quick-add`'s "don't open quick-add over an unsaved
main-window edit" guard — unused by every other host, so it's optional rather than forced on every
caller.

Tests: existing `editPopoverLogic.test.ts` (24 cases) untouched and still green; no new pure logic
was introduced by the bugfix (it's a control-flow change, not new logic), so no new unit tests were
needed for it — the fix is instead exercised end-to-end by `desktop-playwright-tests/e2e/save.spec.ts`,
which byte-diffs the file before/after and asserts **exactly one** line changed (a second silent op
from the old double-apply bug would not have failed that specific assertion, but a duplicate
`edit_text` op is directly visible in `History`/the op log, which `apps/desktop/src-tauri/tests/new_rpcs.rs::op_log_drains_the_stream_into_a_vec` — an existing test — already asserts increments by
exactly one per `Apply`).

## What to open and look at

- `npm run tauri dev`, click a line's pencil, edit it, press Enter. Then run
  `txtodo history <file> --task <id> --limit 5` (or open `.txtodo/oplog.db`) and confirm **one**
  new op landed, not two — this is the regression the bugfix above targets.
- Type a chip (`(A)`, `x`, `+`, `@`, `due:`, etc.) at different caret positions; confirm no
  double-spacing and that `x` swaps in `pri:<P>` correctly for a prioritized line.
- Type a strict-invalid-but-lenient line (e.g. trailing whitespace); confirm the inline error shows
  but Enter still saves.
