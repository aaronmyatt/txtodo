# complete() writes pri: per spec; uncomplete() restores it

todo.txt spec: on completion many clients drop the priority; the spec suggests `pri:A` to preserve it.
Design §2.2 rule 3: txtodo writes `pri:A` rather than inventing a placement. Plan §3.2 popover: `x` chip
"prepends `x <today> ` and, if a priority exists, removes it and appends `pri:<P>`; off, it reverses that."
ADR 0011: `today` is the caller's local date; core takes it as a parameter (no clock in core).

## Exact byte transformations
```
(A) 2026-09-01 Renew passport +admin      --complete(2026-09-11)-->  x 2026-09-11 2026-09-01 Renew passport +admin pri:A
2026-09-01 Renew passport +admin          --complete(2026-09-11)-->  x 2026-09-11 2026-09-01 Renew passport +admin
Renew passport +admin                     --complete(2026-09-11)-->  x 2026-09-11 Renew passport +admin
x 2026-09-11 2026-09-01 Renew … pri:A     --uncomplete()-------->  (A) 2026-09-01 Renew passport +admin
x 2026-09-11 Renew passport pri:B         --uncomplete()-------->  (B) Renew passport
```
`pri:` is appended at the end of the description (after existing tags), matching todo.sh's `do` behaviour
and the M2 parity harness. `uncomplete` removes the `pri:` word plus one preceding SP.

## Edge cases
- Both `(A)` and `pri:B` present on an open line: complete keeps `pri:B`? No: the visible priority wins;
  set `pri:A`, replacing the existing tag in place (via `set_tag`). Document it.
- Lenient `x (A) 2026-09-11 t` then `uncomplete`: result `(A) 2026-09-11 t`? The `(A)` after x is the priority
  by lenient parse, so yes; quirk cleared because the prefix is rebuilt.
- `complete` when `completion_date` would precede `creation_date`: allowed (clocks are the user's problem),
  but `debug_assert!` nothing; the daemon warns via `txtodo doctor` clock sanity.
