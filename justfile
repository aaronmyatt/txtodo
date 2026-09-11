# txtodo task runner. Recipes mirror .claude/budgets.json.commands verbatim (drift audit diffs them).
# just manual: https://just.systems/man/en/
set shell := ["bash", "-euo", "pipefail", "-c"]

# fmt + clippy + typecheck + test + boundaries + file length (what the gate and CI run)
check: fmt lint typecheck test boundaries no-std

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
    rustup run 1.95.0 cargo llvm-cov --workspace --fail-under-lines 80

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

# regenerate the fuzz seed corpora (gitignored) from corpus/*.txt
fuzz-seed:
    rm -rf crates/txtodo-core/fuzz/corpus && mkdir -p crates/txtodo-core/fuzz/corpus/{parse_line,parse_file,slug}
    cat corpus/*.txt | grep -v '^$' | awk '{ print > ("crates/txtodo-core/fuzz/corpus/parse_line/seed-" NR ".txt") }'
    cp corpus/*.txt crates/txtodo-core/fuzz/corpus/parse_file/
    printf 'q4-roadmap' > crates/txtodo-core/fuzz/corpus/slug/valid.txt; printf '../escape' > crates/txtodo-core/fuzz/corpus/slug/traversal.txt

bench:
    cargo bench --workspace

# perf budget: parse_file_100k mean ≤ budgets.json.perf.parse100kMs
bench-check:
    .claude/scripts/check-bench.sh

# core must build with no std at all (plan M1); needs `rustup target add thumbv7em-none-eabihf`
no-std:
    PATH="$(dirname "$(rustup which --toolchain 1.95.0 cargo)"):$PATH" cargo build -p txtodo-core --no-default-features --target thumbv7em-none-eabihf

corpus:
    .claude/scripts/check-corpus-oracle.sh
    cargo test -p txtodo-core --test corpus

release:
    cargo build --workspace --release
