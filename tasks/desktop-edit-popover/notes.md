# Edit popover: single-line CM6, token chips, strict validation via WASM core (plan M7, plan §3.2)

## Goal

Single click on a line opens a popover anchored below it: one single-line CM6 instance (same Lezer
language), pre-filled with the *raw* line including `id:`. Save goes through the daemon. The UI
never re-implements the grammar — validation is the WASM build of `core`.

## Design

- Token chips per plan §3.2: `(A)` `(B)` `(C)` `+` `@` `due:` `t:` `rec:` `x`.
  - Priority chips *replace* the current priority (or remove it when the same chip is tapped).
  - `x` toggles completion: on → prepend `x <today> ` and, if a priority exists, remove it and
    append `pri:<P>` (spec-conformant); off reverses that.
  - Other chips insert the token at the caret with a leading space.
- Strict validation via the WASM core: `txtodo-ffi` exposes `parse_line` (strict) over
  `wasm-bindgen`; the popover loads the wasm module and calls it. Parser errors render inline
  (13 px, danger colour), but the user can still save in lenient mode with a quirk — never block
  saving text. Ref: https://rustwasm.github.io/wasm-bindgen/
- Footer: `Line N · <device>, <relative time>` from the op log (`History`), Cancel and Save.
  Enter saves, Esc cancels. A save identical to the original is a no-op (no op log entry).
- Save path: `Apply(Edit { task: TaskRef { line_number, id }, new_line })` — the two-way address
  lets the daemon reject a stale write (the proto's `TaskRef` contract).

## Acceptance

- Click opens the popover with the raw line including `id:`.
- Enter saves and only that line changes on disk.
- A no-op save (unchanged text) emits no op log entry.

Refs: plan M7 and §3.2 (txtodo-implementation-plan.md), design §7 (txtodo-design.md).
