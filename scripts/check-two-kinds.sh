#!/usr/bin/env bash
# Self-asserting walk through the two kinds of workspace (task two-kinds-of-workspace, ADR 0043),
# for a human to run after a change to lists, repos or the CLI/daemon seam. It starts its own daemon
# on a scratch socket and registry, so it never touches this machine's real txtodo data:
#
#   1. a list: `lists new scratch`, add a line through `--list scratch`, and the line is in
#      `<data dir>/lists/scratch/todo.txt` (txtodo syncs a list between your own devices);
#   2. a repo: in a fresh git repo holding a `todo.txt`, `txtodo add` registers the repo, the line
#      shows up in `git diff todo.txt` (git syncs a repo, so a commit carries it), and `.txtodo/` is
#      kept out of `git status` (the daemon wrote it to the clone's `.git/info/exclude`).
#
# What it does not do: pair two devices (the e2e tests and `scripts/lab` do), or drive MCP (the mcp
# crate's tests do; the CLI and MCP register a git root by the same rule).
#
# Usage: scripts/check-two-kinds.sh   (uses target/debug/{txtodo,txtodod}, building them if missing)
# Override with TXTODO_BIN / TXTODOD_BIN.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)

TXTODO_BIN=${TXTODO_BIN:-$ROOT/target/debug/txtodo}
TXTODOD_BIN=${TXTODOD_BIN:-$ROOT/target/debug/txtodod}
if [ ! -x "$TXTODO_BIN" ] || [ ! -x "$TXTODOD_BIN" ]; then
    echo "check-two-kinds: building txtodo and txtodod"
    cargo build -p txtodo-cli -p txtodo-daemon
fi

fail() {
    echo "check-two-kinds: FAIL -- $*" >&2
    exit 1
}

SCRATCH=$(mktemp -d)
DAEMON_PID=
cleanup() {
    [ -n "$DAEMON_PID" ] && kill "$DAEMON_PID" 2>/dev/null || true
    rm -rf "$SCRATCH"
}
trap cleanup EXIT

# Everything the daemon and the CLI read or write lives under $SCRATCH. The in-memory keystore keeps
# a rebuilt daemon from waiting on a macOS keychain prompt.
export TXTODO_SOCKET="$SCRATCH/state/txtodod.sock"
export TXTODO_REGISTRY_DB="$SCRATCH/state/registry.db"
export TXTODO_CONFIG="$SCRATCH/none.toml"
export TXTODO_TEST_KEYSTORE_MEMORY=1
export TXTODO_NO_AUTOSTART=1
unset TXTODO_TODO_DIR
mkdir -p "$SCRATCH/state" "$SCRATCH/cwd"

"$TXTODOD_BIN" --no-lan --no-relay >"$SCRATCH/daemon.log" 2>&1 &
DAEMON_PID=$!
for _ in $(seq 1 200); do
    [ -S "$TXTODO_SOCKET" ] && break
    kill -0 "$DAEMON_PID" 2>/dev/null || fail "txtodod exited early: $(tail -5 "$SCRATCH/daemon.log")"
    sleep 0.1
done
[ -S "$TXTODO_SOCKET" ] || fail "no daemon socket after 20 s"

txtodo() { (cd "$SCRATCH/cwd" && "$TXTODO_BIN" "$@"); }

echo "check-two-kinds: 1. a list"
txtodo lists new scratch >/dev/null
txtodo --list scratch add "water the fern" >/dev/null
LIST_FILE="$SCRATCH/state/lists/scratch/todo.txt"
grep -q "water the fern" "$LIST_FILE" || fail "the line is not in $LIST_FILE"
txtodo lists | grep -q scratch || fail "'txtodo lists' does not show scratch"

echo "check-two-kinds: 2. a repo"
REPO="$SCRATCH/repo"
mkdir -p "$REPO"
git -C "$REPO" init -q
git -C "$REPO" config user.email check@example.invalid
git -C "$REPO" config user.name check
echo "(A) first line" >"$REPO/todo.txt"
git -C "$REPO" add todo.txt
git -C "$REPO" commit -q -m "a list"
(cd "$REPO" && "$TXTODO_BIN" add "written by txtodo" >/dev/null)
git -C "$REPO" diff todo.txt | grep -q '^+.*written by txtodo' ||
    fail "git diff todo.txt does not show the line txtodo added"
if git -C "$REPO" status --porcelain | grep -q '\.txtodo'; then
    fail ".txtodo/ shows in git status (the clone's exclude was not written)"
fi
grep -qx '\.txtodo/' "$REPO/.git/info/exclude" || fail ".txtodo/ is not in .git/info/exclude"
txtodo workspace list | grep -q "$(basename "$REPO")" || fail "the repo is not a registered workspace"

echo "check-two-kinds: ok -- a list lives in the lists folder, a repo's change is a git diff"
