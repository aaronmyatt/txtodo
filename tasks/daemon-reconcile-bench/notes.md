# criterion bench: reconcile one edit in 10k lines ≤ 20 ms, fail CI (plan M3, plan §5)

`budgets.json.latencyMs = 20` already carries this number with the note "reconcile one external line
edit in a 10k-line file <= 20 ms on the Linux CI runner"; stack.md parks it at tier 4 "until M3".
This task is the promotion. Ref: https://bheisler.github.io/criterion.rs/book/

## Bench
```rust
// crates/txtodo-daemon/benches/reconcile.rs
let old = fixture_10k_with_ids(); let new = edit_line(&old, 5_000, "buy 400 rubber ducks");
c.bench_function("reconcile_10k_one_edit", |b| b.iter(|| reconcile(black_box(&old), black_box(&new), &ctx)));
```
The budget applies to the pure `reconcile` (parse is excluded: it has its own 150 ms/100 k
budget, ≈ 15 ms per 10 k, which would eat most of the 20 ms and measure the wrong thing).
The informational bench shows the end-to-end cost so the 20 ms local round trip claim in
CLAUDE.md §1 has a number behind it even before it is enforced.

## Enforcement
`check-bench.sh` today greps one bencher line and compares to `perf.parse100kMs`. Extend it to a
small table `name → key` and loop (≤ 40 lines still). Expect ~2–4 ms for Myers over 10 k keys;
20 ms is a ceiling for runner noise, same policy as the parse bench: a flap is fixed by making the
code faster or by a human-approved change recorded in `RATCHET.md`, never by retrying.

## Frozen paths touched
`budgets.json`, `check-bench.sh`, `CLAUDE.md`, `stack.md` — every one asks. Land them as one
"semantic machinery" commit (constitution §6), separate from the bench itself.

## As built (2026-09-12)
`benches/reconcile.rs`: `reconcile_10k_one_edit` 46.8 ms before, **12.1 ms** after the fast id
scan and lazy id recovery (`perf(daemon)` commit). Attribution: `diff_lines` 9.0 ms (core), full
parse per line 4.5 ms per 10k. **CI enforcement is not wired**: `check-bench.sh`, `budgets.json.perf`
and the `CLAUDE.md` row are frozen paths — the human runs those edits (this notes file names the
bench and the number they need).
