# criterion bench: parse 100k lines ≤ 150 ms, fail CI if exceeded

Plan M1: "Benchmarks (`criterion`): parse 100 k lines; budget ≤ 150 ms on the CI runner." Plan §5: performance
budgets fail CI when exceeded, measured on the Linux runner. Ref: https://bheisler.github.io/criterion.rs/book/

## Bench
```rust
// benches/parse.rs
fn hundred_k() -> Vec<u8> { /* cycle corpus/*.txt task lines to 100_000 lines, LF */ }
c.bench_function("parse_file_100k", |b| b.iter(|| txtodo_core::parse_file(black_box(&bytes))));
c.bench_function("parse_line_100k_lenient", |b| b.iter(|| for l in lines { black_box(parse_line(l, Mode::Lenient)); }));
c.bench_function("tokenize_100k", …);
```
Budget applies to `parse_file_100k` (it includes line splitting and per-line parse).

## Enforcing in CI (tier 2)
`cargo bench -- --output-format bencher` prints `test parse_file_100k ... bench: 123456 ns/iter`. A ≤ 40-line
script `.claude/scripts/check-bench.sh` greps that line and compares to `budgets.json.perf.parse100kMs`.
Runner noise: CI runners vary ±30 %; 150 ms is a ceiling, expect ~40 ms. If it flaps, the fix is a faster
parser or a human-approved budget change in `RATCHET.md` (Exceptions granted), never a retry loop.

## Local
`just bench` exists (M0). Add `just bench-check` that runs the script. macOS numbers are informational.
