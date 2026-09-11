# Stack mapping — txtodo

Rust 1.95 (edition 2024) Cargo workspace, 12 crates. Derived by `/setup` on 2026-09-11 from
`.claude/budgets.json` and the constitution (template 2.0.0). Frozen path.

## Toolchain

| Concern | Tool | Version | Installed by /setup |
|---|---|---|---|
| Format | rustfmt (`cargo fmt`) | 1.95 toolchain | no (present) |
| Lint | clippy | 1.95 toolchain | no (present) |
| Typecheck | rustc via `cargo check` | 1.95 | no (present) |
| Tests | `cargo test` | 1.95 | no (present) |
| Coverage | cargo-llvm-cov | 0.9.1 | `brew install cargo-llvm-cov` + `rustup component add llvm-tools-preview` |
| Licences / advisories | cargo-deny | 0.20.2 | `brew install cargo-deny` |
| Task runner | just | 1.58.0 | already present |
| JSON reads in scripts | node | 24 | no (present) |

## Rule → tier

| Rule | Tier | Enforced by | Number |
|---|---|---|---|
| Function length | 1 | clippy [`too_many_lines`](https://rust-lang.github.io/rust-clippy/master/index.html#too_many_lines), `clippy.toml too-many-lines-threshold` | 60 |
| File length | 3 | `.claude/scripts/check-file-length.sh` | 400 |
| Change size | gate + CI | `gate.sh` (git diff numstat) and CI PR-size step | 300 |
| Params | 1 | clippy [`too_many_arguments`](https://rust-lang.github.io/rust-clippy/master/index.html#too_many_arguments), `too-many-arguments-threshold` | 5 |
| Nesting | 1 | clippy [`excessive_nesting`](https://rust-lang.github.io/rust-clippy/master/index.html#excessive_nesting), `excessive-nesting-threshold` = 5 | 3 inside a method (clippy counts `impl` + `fn` blocks; found on 2026-09-11 when a `for`+`if` inside a method tripped 3) |
| Complexity | 1 | clippy [`cognitive_complexity`](https://rust-lang.github.io/rust-clippy/master/index.html#cognitive_complexity) (nursery, enabled by name), `cognitive-complexity-threshold` | 10 |
| Line width | 2 | [`rustfmt.toml max_width`](https://rust-lang.github.io/rustfmt/#max_width) | 100 |
| Assertions / fn | 3 | `.claude/scripts/check-assertions.sh` (report only) | 2 |
| Coverage | 2 | [`cargo llvm-cov --fail-under-lines`](https://github.com/taiki-e/cargo-llvm-cov#usage) | 80 (promoted 2026-09-11 from the measured 0; workspace measured 97.07%) |
| Boundaries (§2) | 3 | `.claude/scripts/check-boundaries.sh` against `budgets.json.slices.allowedDeps` | — |
| Exhaustive match (§3) | 1 | rustc: `match` is exhaustive by construction; [`non_exhaustive_omitted_patterns`](https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html) not enabled | — |
| Immutability (§3) | 1 | rustc: bindings immutable unless `mut`; `unused_mut` under `-D warnings` | — |
| Unchecked errors (§3) | 1 | rustc [`unused_must_use`](https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html#unused-must-use) on `Result`; clippy `unwrap_used`, `expect_used` deny | — |
| Swallowed errors (§3) | 4 | `let _ = fallible()` is legal Rust; review greps for `let _ =` | — |
| No print / todo / dbg | 1 | clippy `print_stdout`, `print_stderr` (CLI allows), `todo`, `unimplemented`, `dbg_macro` | — |
| Public docs | 1 | rustc [`missing_docs`](https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html#missing-docs) deny | — |
| Unsafe | 1 | `#![forbid(unsafe_code)]` in every crate root except `txtodo-ffi` | — |
| Dead code | 1 | rustc `dead_code` under `-D warnings` | — |
| Licences, advisories | 2 | [`cargo deny check`](https://embarkstudios.github.io/cargo-deny/checks/index.html), `deny.toml` | — |
| Property tests (§7) | 1 | [`proptest`](https://docs.rs/proptest) (plan M1) — added when core lands, not by /setup | — |

## Commands

From `budgets.json.commands`. Proven 2026-09-11: clean tree / deliberately broken tree.

| Key | Command | Clean | Broken |
|---|---|---|---|
| format | `cargo fmt --all --check` | 0 | 1 (mis-indented fn) |
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 | 1 (fn with 6 params) |
| typecheck | `cargo check --workspace --all-targets` | 0 | 1 (`-> u8 { "no" }`) |
| test | `cargo test --workspace` | 0 | 1 (canary test) |
| testCoverage | `rustup run 1.95.0 cargo llvm-cov --workspace --fail-under-lines 80` | 0 (97.07% measured) | not exercised since promotion |
| boundaries | `.claude/scripts/check-boundaries.sh` | 0 | 1 (query → store edge) |
| fileLength | `.claude/scripts/check-file-length.sh` | 0 | 1 (401-line file) |
| assertions | `.claude/scripts/check-assertions.sh` | 0 | 0, reported `fn naked has 0 (min 2)` |
| deny | `cargo deny check` | 0 | 1 (wildcard path deps before `publish = false`) |
| feedback.format | `rustfmt --edition 2024 --check {file}` | per file | — |
| feedback.lint | whole-workspace clippy (clippy has no single-file mode) | — | — |
| baselinePrune | `null` — no baseline exists (greenfield, zero violations) | — | — |

Canary: `budgets.json.canary` = copy `.claude/canary/failing_test.rs` to
`crates/txtodo-query/tests/__setup_canary__.rs`, run the gate, expect block, delete the copy.

## Boundary check

`.claude/scripts/check-boundaries.sh`, quoted in full:

```bash
#!/usr/bin/env bash
# Tier-3 slice fence (non-interactive form): each crate's [dependencies]/[dev-dependencies] may
# name only the workspace crates listed for it in budgets.json.slices.allowedDeps. Exits 1 on any
# extra edge. Cargo manifest format: https://doc.rust-lang.org/cargo/reference/manifest.html
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
status=0
for manifest in "$ROOT"/crates/*/Cargo.toml; do
  crate=$(basename "$(dirname "$manifest")")
  # workspace crates referenced in any dependency table of this manifest
  actual=$(awk '/^\[(dev-|build-)?dependencies/{on=1;next} /^\[/{on=0} on && /^txtodo-/{sub(/[ =.].*/,""); print}' "$manifest" | sort -u)
  allowed=$(node -p 'const s=JSON.parse(require("fs").readFileSync(process.argv[1])).slices.allowedDeps;
    (s[process.argv[2]]||[]).sort().join("\n")' "$ROOT/.claude/budgets.json" "$crate")
  extra=$(comm -23 <(echo "$actual" | sed '/^$/d') <(echo "$allowed" | sed '/^$/d') || true)
  if [ -n "$extra" ]; then
    echo "boundary: $crate depends on $(echo "$extra" | tr '\n' ' ')— not in budgets.json allowedDeps"; status=1
  fi
done
exit $status
```

## Not mechanically enforced

- **Response payload: N/A.** No HTTP surface until M6 (libraries, CLI, daemon over a unix socket). Re-justify when `txtodo-mcp` lands.
- **Local round trip (20 ms):** tier 4 until M3. The plan's perf budgets become criterion benches in CI at M3 (`reconcile one edit in 10k lines ≤ 20 ms`).
- **Swallowed errors:** `let _ = fallible()` and `.ok()` discard are legal; review greps for them. No clippy lint covers the intent without false positives.
- **Bounded loops / collections:** review + assert. No mechanical form.
- **Assertions ≥ 2:** heuristic reports, review decides (D3).
- **Coverage floor 80:** promoted from the measured 0 on 2026-09-11 once M1 core tests landed (97.07% measured).

## Idioms

- **Global state to avoid:** `static mut`, `lazy_static!`/`OnceLock` holding mutable state, `thread_local!` used as a store, a process-wide `Arc<Mutex<_>>` passed by clone into everything. Inject instead.
- **Fakes:** clock → a `Clock` trait with a fixed impl in tests; randomness → seeded `rand::rngs::StdRng`; network → in-process transports (the M4 sync simulator); filesystem → `tempfile::tempdir()` plus the `FileActor`'s injected paths.
- **Property testing:** `proptest`. Fuzzing: `cargo-fuzz` (nightly; M0 task, not machinery).
- **Generated artifacts (diff-budget exempt, committed alone):** `Cargo.lock`, protobuf output in `txtodo-proto`, the Lezer grammar in `apps/desktop` (M7), any future baseline under `budgets.json.baselinePaths`.

## Judgement calls

- **Slice = Cargo crate; "no cross-slice import" = plan §2 edge allowlist.** The constitution bans slice imports outright. The plan defines a layered crate graph and says it wins. The boundary script enforces the declared edges. Rejected: one giant crate with `features/` folders (loses `cargo`'s own compile-time boundary).
- **Per-crate `Cargo.toml` is not frozen.** The edge allowlist in `budgets.json` is; the script guards the edges. Rejected: freezing all 12 manifests (every M0 dependency add would ask).
- **Coverage floor = 0 (measured), not 80 (wanted).** Skill rule: floor is today's number. 80 is the ratchet target in `RATCHET.md`. Rejected: 80 now, which blocks every stop until M1.
- **Coverage command via `rustup run 1.95.0`.** Homebrew's `cargo` lacks `llvm-profdata`. Only this command is pinned; the rest use `cargo` so CI's rustup honours `rust-toolchain.toml`. Rejected: changing the human's PATH.
- **`cognitive_complexity` stands in for cyclomatic.** Clippy ships no cyclomatic lint; cognitive complexity at the same threshold is the nearest native rule. Rejected: a tier-3 branch-counting awk (worse signal than the native lint).
- **`publish = false` on every crate.** cargo-deny treats version-less path deps as wildcards for publishable crates. Nothing is published yet. Revisit when the first crate ships to crates.io.
- **feedback.lint runs whole-workspace clippy.** Clippy cannot lint one file. Cost: a few seconds per write. Rejected: skipping lint feedback.
- **No baseline files.** Greenfield: zero violations, so `baselinePaths` is empty and `baselinePrune` is null. The fence's deny rule has nothing to guard until `/ratchet` adds a measurement tool.
- **Gate loop guard.** Three identical failing rounds (`.git/setup-gate-strikes`) and the gate stops re-blocking with a notice. Rejected: block forever (an unattended Pi session would burn tokens with no human to decide). Seen live on 2026-09-11.
- **Bash writes and the fence.** Claude Code's Bash tool can write any path. `fence.sh` asks when a command has a write operator (`>`, `sed -i`, `tee`, `mv`, `rm`, `cp`) and names a frozen path literal; denies for a baseline path. Heuristic, not airtight: prefer Edit/Write for machinery files. Pi's fence sees only `write`/`edit`, so the same gap exists for its `bash` tool.
- **Nesting threshold 5, not 3.** clippy's `excessive_nesting` counts the `impl` and `fn` blocks; 5 gives three real levels inside a method, four inside a free function. Rejected: 3 (no method may contain a loop with a branch).
- **Windows CI skips tier-3 scripts.** They are bash; see `RATCHET.md` priority 3.

## Framework notes

- None. No framework bends a rule at M0. Revisit at M7 (Tauri + Svelte add a second stack under `apps/desktop`, which needs its own mapping).
