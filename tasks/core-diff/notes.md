# diff_lines (Myers, by id when present, else by content) and diff_text (char-level)

Plan M1: `diff_lines(a: &File, b: &File) -> Vec<LineDiff>` and `diff_text(a: &str, b: &str) -> Vec<TextEdit>`.
Consumers: M3's reconciler (external edit → ops), M4's CRDT text merges, M6's `dry_run` unified diff.

## diff_lines
Key per line: `Key::Id(Ulid)` when the line has a valid `id:` tag, else `Key::Content(u64 hash of raw)`.
Blank lines are `Key::Blank(index-within-run)` so a moved blank stays a blank, not a Change.
Run Myers on the key sequences (Ref: Myers 1986, "An O(ND) Difference Algorithm";
https://neil.fraser.name/writing/diff/myers.pdf). Then classify:
- same key both sides, same raw → `Keep`; same id, different raw → `Change { from, to }`
- a key deleted at i and inserted at j → `Move { from: i, to: j }` (only for `Id` keys; content keys stay
  Delete+Insert because two identical lines are indistinguishable)
`LineDiff` carries indices into `a` and `b` so the reconciler can map to line numbers.

## diff_text
Character-level (Unicode scalar values, not bytes), because the CRDT text type (Loro) addresses chars.
Also Myers, on `Vec<char>`; output merged into runs: `[Insert { at: 5, text: "abc" }, Delete { at: 12, len: 3 }]`.
Positions refer to the *original* string, applied in order.

## Budgets
Myers in ≤ 60 lines is tight but fine for the greedy forward version with a `Vec<isize>` V array; bound the
loop by `max_d = a.len() + b.len()` and `debug_assert!(d <= max_d)`. No recursion (constitution §3).
