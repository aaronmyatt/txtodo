# txtodo task runner. Recipes mirror .claude/budgets.json.commands verbatim (drift audit diffs them).
# just manual: https://just.systems/man/en/
set shell := ["bash", "-euo", "pipefail", "-c"]

# fmt + clippy + typecheck + test + boundaries + file length (what the gate and CI run)
check: fmt lint typecheck test boundaries

fmt:
    cargo fmt --all --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

typecheck:
    cargo check --workspace --all-targets

test:
    cargo test --workspace

# line coverage against the floor in budgets.json (rustup toolchain: Homebrew cargo lacks llvm-profdata)
coverage:
    rustup run 1.95.0 cargo llvm-cov --workspace --fail-under-lines 0

boundaries:
    .claude/scripts/check-boundaries.sh
    .claude/scripts/check-file-length.sh
    .claude/scripts/check-assertions.sh
    .claude/scripts/check-specs-mirror.sh

deny:
    cargo deny check

# fuzz <target> <secs>: plan M0 wants this; cargo-fuzz needs nightly and is installed at M1, not by /setup
fuzz target secs="60":
    PATH="$(dirname "$(rustup which --toolchain nightly cargo)"):$PATH" cargo fuzz run --fuzz-dir crates/txtodo-core/fuzz {{target}} -- -max_total_time={{secs}}

bench:
    cargo bench --workspace

corpus:
    .claude/scripts/check-corpus-oracle.sh

release:
    cargo build --workspace --release
