# Lenient mode with a Quirks enum

Design §2.3: lenient mode is the default when *reading*, never when writing; each leniency is recorded on
the parsed task as a quirk so it round-trips (rule 2). `txtodo lint` reports quirks; `txtodo fmt` fixes them.

## Quirk table (bits in `Quirks`; names are the spec's)
| Input | Bit | Parsed as |
|---|---|---|
| `x no completion date here` | `NO_COMPLETION_DATE` | completed, `completion_date: None`, description `no completion date here` |
| `x (A) 2026-09-11 t` | `PRIORITY_AFTER_X` | completed, priority A, completion date set |
| `x 2026-09-11 (A) t` | `PRIORITY_AFTER_DATE` | completed, priority A |
| `2026-09-11\ttab\twords` | `TABS` | creation date set; description keeps the tabs |
| `2026-09-11 t   ` | `TRAILING_WS` | description keeps the spaces |
| `ref:../x` | `INVALID_REF` | (task core-task-views) treated as no ref |
| LF and CRLF in one file | `MIXED_ENDING` | (task core-parse-file) per line |

## Contract
- `parse_line(raw, Mode::Lenient)` is total over `&str`: it never returns `Err`. Anything the prefix parser
  cannot place becomes description. Fuzzing (core-fuzz-targets) enforces "never panics"; this task enforces
  "never errs" with a test over every corpus line.
- Strict and lenient share one prefix parser parameterised by a `Leniency` policy struct, not two copies
  (constitution: flag duplication, but a policy struct is the plain-function way here).
- `Line.quirks` is what the formatter later preserves: a lenient line is written back byte-identical unless
  an `Edit` touched the quirky field.
