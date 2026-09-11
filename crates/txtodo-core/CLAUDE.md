# txtodo-core

## Purpose
Parser, tokenizer, model, byte-preserving formatter, diff. Plan M1.

## Public interface
`parse_line`, `tokenize`, `parse_file`, `File::to_bytes`, `Edit`/`apply`, `diff_lines`, `diff_text` (shape frozen in plan M1).

## Invariants
- No I/O, no clocks, no async dependency. `no_std` + `alloc` must build.
- `format(parse(x)) == x` for every corpus line; `tokenize` covers `[0, len)` exactly.
- Never emits a construct the todo.txt spec does not define.
- May depend only on: nothing in the workspace.
