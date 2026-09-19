#!/usr/bin/env bash
# Detector for tasks/test-registry-leak-cleanup: fails if any crate's test suite writes into this
# machine's real txtodo data directory instead of a test-injected one. `$XDG_DATA_HOME` is pointed
# at one scratch dir for the whole run; anything left under it afterward is a leak straight into
# `~/.local/share/txtodo/registry.db` (the incident this guards against — see that task's
# notes.md: 124 dead workspace-registry rows, mostly from Playwright e2e runs before commit
# 0e59c22 gave the e2e daemon its own `TXTODO_E2E_GLOBAL_DIR`). `$HOME` is deliberately left alone:
# cargo/rustup need the real one to find the toolchain and their own registry cache, so it can't be
# redirected for the whole `cargo test` invocation the way `$XDG_DATA_HOME` can — a test that
# writes into the real `$HOME` (e.g. `~/Library/LaunchAgents`) is a narrower, separate concern
# (tasks/test-registry-leak-cleanup's own "audit launchd/systemd use" line), not this script's job.
#
# Usage: scripts/check-test-hermetic.sh [-- <extra cargo test args>]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT

echo "check-test-hermetic: XDG_DATA_HOME=$SCRATCH cargo test --workspace $*"
XDG_DATA_HOME="$SCRATCH" cargo test --workspace "$@"

LEAKED=$(find "$SCRATCH" -type f 2>/dev/null || true)
if [ -n "$LEAKED" ]; then
    echo "check-test-hermetic: FAIL -- a test wrote into the scratch XDG_DATA_HOME (should stay empty):" >&2
    echo "$LEAKED" >&2
    exit 1
fi

echo "check-test-hermetic: ok -- no test reached the real machine's XDG_DATA_HOME"
