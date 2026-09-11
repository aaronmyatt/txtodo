# cargo-fuzz targets: parse_line (both modes), parse_file, slug validator

Plan M1: fuzz targets `parse_line` (both modes) and `parse_file`; acceptance: 3600 s each with zero crashes
locally, 60 s in CI (job `fuzz`, M0). Plan §5 security checklist: "path traversal impossible via `ref:` (fuzz
the slug validator)". The M0 stub in `crates/txtodo-core/fuzz/` is replaced, not extended.

## Targets
```rust
// parse_line.rs
fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else { return };
    let lenient = txtodo_core::parse_line(s, Mode::Lenient);
    assert!(lenient.is_ok(), "lenient must be total");
    if let Ok(strict) = txtodo_core::parse_line(s, Mode::Strict) { assert_eq!(fields(&strict), fields(&lenient.unwrap())); }
    let spans = txtodo_core::tokenize(s); assert_contiguous(&spans, s.len());
});
// parse_file.rs
fuzz_target!(|data: &[u8]| { assert_eq!(txtodo_core::parse_file(data).to_bytes(), data); });
// slug.rs
fuzz_target!(|data: &[u8]| { if let Ok(s) = core::str::from_utf8(data) {
    if txtodo_core::is_valid_slug(s) { assert!(!s.contains('/') && s != "." && s != ".." && s.len() <= 64); } } });
```
`unwrap` is fine inside fuzz targets (they are tests; the fuzz crate is outside the workspace lints anyway).

## Seeds
`fuzz/corpus/parse_line/` one file per corpus line (script it once from `corpus/*.txt`; the directory is
gitignored, regenerate with `just fuzz-seed`, add that recipe).

## Record of the 3600 s runs
| Target | Date | Runs | Result |
|---|---|---|---|
| parse_line | 2026-09-12 | 111,975,023 | 0 crashes; 2 slow-units kept (not crashes) |
| parse_file | 2026-09-12 | 52,020,955 | 0 crashes; 3 slow-units kept (not crashes) |
| slug | — | — | not run (outside this task's scope) |

Both runs were `just fuzz <target> 3600` on the local machine, slowed by concurrent agent load:
parse_line counted 4313 s, parse_file 3601 s. `-max_total_time` is fuzz time, so the wall-clock
total exceeded 3600 s. No `crash-*`, `timeout-*` or `oom-*` artifacts were produced; the
`slow-unit-*` files under `fuzz/artifacts/<target>/` (gitignored) are inputs libFuzzer flagged as
slow, not failures.
