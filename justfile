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

# cargo-nextest: ~10x faster than `cargo test` on this workspace (task rust-build-speed follow-up,
# 1.33s vs 13.46s on txtodo-core's 69 tests) — no doc-tests in this workspace to lose by switching.
# https://nexte.st/docs/installation/pre-built-binaries/
test:
    @command -v cargo-nextest >/dev/null || { echo "cargo-nextest is not installed: run \`just install-nextest\` (or \`cargo install cargo-nextest --locked\`)"; exit 1; }
    cargo nextest run --workspace

# The test runner `just test`, the agent gate (budgets.json commands.test) and CI all use (task
# nextest-adoption): one process per test, so a test that leans on process-global state (sockets,
# env, the registry) behaves the same everywhere. https://nexte.st/docs/installation/pre-built-binaries/
install-nextest:
    cargo install cargo-nextest --locked

# line coverage against the floor in budgets.json (rustup toolchain: Homebrew cargo lacks llvm-profdata)
coverage:
    rustup run 1.95.0 cargo llvm-cov --workspace --fail-under-lines 80

boundaries:
    .claude/scripts/check-boundaries.sh
    .claude/scripts/check-file-length.sh
    .claude/scripts/check-assertions.sh
    .claude/scripts/check-specs-mirror.sh
    scripts/check-version-sync.sh

deny:
    cargo deny check

# the full crdt-sync-simulator sweep (plan M4 acceptance: 1000 runs, random seeds each time);
# `cargo test` alone only runs 20 fixed seeds — this is deliberately not part of `check`.
# TXTODO_SIM_SEED=<n> reruns exactly one seed to reproduce a failure this prints.
sim:
    cargo test -p txtodo-crdt --release --test sim -- --ignored --nocapture

# fuzz <target> <secs>: plan M0 wants this; cargo-fuzz needs nightly and is installed at M1, not by /setup
fuzz target secs="60":
    PATH="$(dirname "$(rustup which --toolchain nightly cargo)"):$PATH" cargo fuzz run --fuzz-dir crates/txtodo-core/fuzz {{target}} -- -max_total_time={{secs}}

# regenerate the fuzz seed corpora (gitignored) from corpus/*.txt
fuzz-seed:
    rm -rf crates/txtodo-core/fuzz/corpus && mkdir -p crates/txtodo-core/fuzz/corpus/{parse_line,parse_file,slug}
    cat corpus/*.txt | grep -v '^$' | awk '{ print > ("crates/txtodo-core/fuzz/corpus/parse_line/seed-" NR ".txt") }'
    cp corpus/*.txt crates/txtodo-core/fuzz/corpus/parse_file/
    printf 'q4-roadmap' > crates/txtodo-core/fuzz/corpus/slug/valid.txt; printf '../escape' > crates/txtodo-core/fuzz/corpus/slug/traversal.txt

# Opt-in, not the default build path (task rust-build-speed follow-up): sccache only caches
# non-incremental compiles, so this disables incremental for the run — a real ~12x win on a
# rebuild whose crate content already sits in the cache (measured 4.93s vs 58.97s for
# txtodo-daemon after `cargo clean -p txtodo-daemon`), but the normal edit-one-file loop wants
# incremental compilation instead, not this. Use when starting fresh in a new worktree/branch
# that shares dependency versions with one already built elsewhere on this machine.
# https://github.com/mozilla/sccache
cold-build *args:
    sccache --start-server 2>/dev/null || true
    RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo build {{args}}

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

# Local pre-tag smoke: build the three release binaries for every release.yml matrix leg via
# cargo-zigbuild (https://github.com/rust-cross/cargo-zigbuild), assert the musl legs are static
# (no `ldd` interpreter), sign every artifact with cosign (https://docs.sigstore.dev/cosign/) using
# a local key (release.yml itself signs keyless via GitHub OIDC — see RELEASE_CI.patch.md — this
# recipe is a pre-tag *local* smoke, so it needs a key pair, generated once with
# `cosign generate-key-pair`), verify every signature, and generate the SBOM via cargo-cyclonedx
# (https://github.com/CycloneDX/cyclonedx-rust-cargo). Mirrors deploy/nix/packages.nix's bin-name
# mapping (crate name -> [[bin]] name): txtodo-cli -> txtodo, txtodo-daemon -> txtodod,
# txtodo-tui -> txtodo-tui. Windows ships txtodo-tui only, matching ci.yml's own Windows exclusion
# of txtodo-cli/txtodo-daemon/txtodo-mcp under ADR 0010 (no Unix domain sockets on Windows).
release:
    #!/usr/bin/env bash
    set -euo pipefail
    rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-gnu
    mkdir -p dist
    build() {
        local zig_target="$1" rust_target="$2"; shift 2
        for pkg in "$@"; do
            cargo zigbuild --release --target "$zig_target" -p "$pkg" --locked
            case "$pkg" in
                txtodo-cli) bin=txtodo ;;
                txtodo-daemon) bin=txtodod ;;
                txtodo-tui) bin=txtodo-tui ;;
            esac
            ext=""; case "$rust_target" in *windows*) ext=".exe" ;; esac
            cp "target/${rust_target}/release/${bin}${ext}" "dist/${bin}-${zig_target%%.*}${ext}"
        done
    }
    build x86_64-unknown-linux-gnu.2.17 x86_64-unknown-linux-gnu txtodo-cli txtodo-daemon txtodo-tui
    build x86_64-unknown-linux-musl x86_64-unknown-linux-musl txtodo-cli txtodo-daemon txtodo-tui
    build aarch64-unknown-linux-musl aarch64-unknown-linux-musl txtodo-cli txtodo-daemon txtodo-tui
    # Same zig target as rust_target here (not a typo left in): windows-gnu has no
    # glibc-version-suffix convention, and "x86_64-windows-gnu" alone isn't a real target triple.
    build x86_64-pc-windows-gnu x86_64-pc-windows-gnu txtodo-tui
    echo "== static check: musl artifacts must have no dynamic interpreter =="
    for f in dist/*musl*; do
        file "$f" | grep -q 'statically linked' || { echo "::error:: $f is not static"; exit 1; }
    done
    echo "== SBOM: cargo cyclonedx --all, merged via deploy/release/merge-bom.jq =="
    # cargo-cyclonedx has no workspace-wide mode: `--all` writes one <crate>.cdx.json per
    # workspace member next to its own Cargo.toml. merge-bom.jq combines the release-relevant
    # ones (the 3 shipped bins + their full dep graph) into one bom.json.
    cargo cyclonedx --all --format json
    jq -s -f deploy/release/merge-bom.jq \
        crates/txtodo-cli/txtodo-cli.cdx.json crates/txtodo-daemon/txtodo-daemon.cdx.json \
        crates/txtodo-tui/txtodo-tui.cdx.json crates/txtodo-core/txtodo-core.cdx.json \
        crates/txtodo-query/txtodo-query.cdx.json crates/txtodo-model/txtodo-model.cdx.json \
        crates/txtodo-store/txtodo-store.cdx.json crates/txtodo-crdt/txtodo-crdt.cdx.json \
        crates/txtodo-sync/txtodo-sync.cdx.json crates/txtodo-proto/txtodo-proto.cdx.json \
        crates/txtodo-mcp/txtodo-mcp.cdx.json \
        > bom.json
    echo "== sign every artifact (local key — generate once with: cosign generate-key-pair) =="
    # --bundle alone carries the signature (cosign 3.x deprecates the separate --output-signature
    # file in favour of the bundle: https://docs.sigstore.dev/cosign/signing/overview/).
    for f in dist/* bom.json; do
        cosign sign-blob --key cosign.key --yes --bundle "${f}.bundle" "$f"
        cosign verify-blob --key cosign.pub --bundle "${f}.bundle" "$f"
    done
    echo "release smoke OK: $(ls dist/) bom.json"

# Tagged releases install via scripts/install.sh or brew; this is for pre-release builds.
# Full install of this checkout (binaries + desktop app, self-signed, service restarted)
install:
    scripts/install-local.sh

# task desktop-daemon-sidecar-bundle: stages this host's own txtodod build as apps/desktop's Tauri
# sidecar (tauri.conf.json's bundle.externalBin), so a local `npm run tauri build` produces a
# bundle with a working daemon instead of relying on $PATH. Host-only (one target triple, via
# `rustc --print host-tuple`, https://v2.tauri.app/develop/sidecar/#platform-specific-binaries) —
# .github/workflows/release.yml's build-desktop job does the equivalent per matrix leg for every
# shipped platform, not just the CI runner's own host.
stage-desktop-sidecar:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p txtodo-daemon --bin txtodod
    triple=$(rustc --print host-tuple)
    ext=""; case "$triple" in *windows*) ext=".exe" ;; esac
    mkdir -p apps/desktop/src-tauri/binaries
    cp "target/release/txtodod${ext}" "apps/desktop/src-tauri/binaries/txtodod-${triple}${ext}"
    echo "staged apps/desktop/src-tauri/binaries/txtodod-${triple}${ext}"

# `--bundles app` skips the DMG (a second copy): https://v2.tauri.app/reference/cli/#build
# `cargo metadata` gives the real target dir, worktree or not:
# https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
# `ditto` keeps the code signature and extended attributes, `cp -R` may not: https://ss64.com/mac/ditto.html
# `osascript` asks a running copy to quit before it is replaced: https://ss64.com/mac/osascript.html
# Build the desktop app, install it over /Applications/txtodo.app, delete the target/ copy (macOS only)
install-desktop: stage-desktop-sidecar
    #!/usr/bin/env bash
    set -euo pipefail
    target=$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)
    (cd apps/desktop && npm run tauri build -- --bundles app)
    built="$target/release/bundle/macos/txtodo.app"
    [ -d "$built" ] || { echo "no bundle at $built"; exit 1; }
    osascript -e 'if application id "com.txtodo.desktop" is running then tell application id "com.txtodo.desktop" to quit'
    rm -rf /Applications/txtodo.app
    ditto "$built" /Applications/txtodo.app
    rm -rf "$built"
    echo "installed /Applications/txtodo.app (removed $built)"

# Install `txtodo` + `txtodod` to $CARGO_HOME/bin (default ~/.cargo/bin): a path that survives
# `cargo clean` and worktree removal, unlike target/, which is where a launchd/systemd unit ended up
# pointing at (a deleted or rebuilt-under-it binary). Both, not just the daemon: `txtodo daemon
# install` records the `txtodod` sitting *beside* the `txtodo` that runs it. Leaves the running
# daemon alone; `just repoint-service` is the separate, one-restart step.
# https://doc.rust-lang.org/cargo/commands/cargo-install.html
install-daemon:
    cargo install --path crates/txtodo-daemon --locked --force --target-dir target/install
    cargo install --path crates/txtodo-cli --locked --force --target-dir target/install

# Point the real launchd/systemd unit at the `install-daemon` copy. Restarts the daemon once
# (bootout, rewrite unit, bootstrap+kickstart). `env -u`: run without TXTODO_NO_SERVICE, which
# .cargo/config.toml sets for anything started through cargo but which would refuse this on purpose.
repoint-service:
    #!/usr/bin/env bash
    set -euo pipefail
    bin="${CARGO_HOME:-$HOME/.cargo}/bin"
    [ -x "$bin/txtodo" ] && [ -x "$bin/txtodod" ] || { echo "run 'just install-daemon' first"; exit 1; }
    ctl() { env -u TXTODO_NO_SERVICE "$bin/txtodo" daemon "$@"; }
    ctl stop || true
    ctl install --force
    ctl start
    ctl status

# By-hand check (tasks/relay-id-keystore): starts the installed txtodod twice on the macOS login
# keychain and prints PASS when the relay node id is the same both times. Touches one real keychain
# item; see the script's header. `timeout` is seconds per start, time to answer a keychain prompt.
# Check the relay node id survives a restart on the macOS keychain (prints PASS/FAIL)
check-relay-id-keychain timeout="180":
    scripts/check-relay-id-keychain.sh --timeout-s {{timeout}}

# Desktop visual goldens (tasks/desktop-visual-regression): the light and dark Playwright projects
# against the browser mock, which starts its own dev server. `goldens-check` compares without
# writing; `goldens-update` rewrites the PNGs (review them before committing: `goldens-review`).
# Goldens are per-OS: this writes *-darwin.png on a Mac, the nightly (ubuntu) reads *-linux.png.
# https://playwright.dev/docs/test-snapshots#updating-screenshots
# Compare the light/dark visual goldens without writing
goldens-check:
    cd apps/desktop && TXTODO_TEST_KEYSTORE_MEMORY=1 TXTODO_NO_SERVICE=1 npx playwright test --project=light --project=dark

# Regenerate the light/dark visual goldens (review before committing)
goldens-update:
    cd apps/desktop && TXTODO_TEST_KEYSTORE_MEMORY=1 TXTODO_NO_SERVICE=1 npx playwright test --project=light --project=dark --update-snapshots

# https://ss64.com/mac/open.html
# Open every golden in Preview and list which ones changed
goldens-review:
    git status --short -- apps/desktop/e2e/visual
    open apps/desktop/e2e/visual/*-snapshots/*.png
