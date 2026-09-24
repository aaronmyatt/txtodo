#!/usr/bin/env bash
# By-hand check for tasks/relay-id-keystore line 1: with `--key-store os` (the macOS login
# keychain), txtodod's relay node id is the same across two starts. Prints PASS or FAIL.
#
# What it touches: a throwaway workspace under /tmp (removed on exit) and ONE real keychain item,
# `txtodo` / `device/relay-identity`. That item is this device's relay identity, the same one the
# launchd daemon uses under the `auto` default: the check reads it, or writes it if it is missing.
# The running launchd daemon is not stopped; this daemon has its own --dir socket and pidfile.
#
# macOS may show "txtodod wants to use your confidential information stored in txtodo". Answer it
# (Always Allow). Ad-hoc signed builds ask again after every reinstall (see the task's notes.md).
#
# The relay URL is loopback and nothing listens there: binding the endpoint mints/loads the id
# without needing a reachable relay or the network (same trick as tests/relay_node_id.rs).
#
# Usage: scripts/check-relay-id-keychain.sh [--timeout-s N]
# Needs: txtodod and txtodo on PATH or in ~/.cargo/bin (or set $TXTODOD / $TXTODO).
# Ref: https://developer.apple.com/documentation/security/keychain_services/access_control_lists
set -euo pipefail

TIMEOUT_S=180
while [ $# -gt 0 ]; do
    case "$1" in
        --timeout-s) TIMEOUT_S=$2; shift 2 ;;
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

# Resolves a binary: $override if set, else PATH, else ~/.cargo/bin (where `just install` and the
# launchd job put them; a non-login shell may not have it on PATH). Fails early with a clear line.
# Ref: https://www.gnu.org/software/bash/manual/html_node/Bash-Builtins.html#index-command
resolve_bin() {
    local name=$1 override=$2  # $3: the env var that overrides it, named in the error
    if [ -n "$override" ]; then printf '%s\n' "$override"; return; fi
    if command -v "$name" >/dev/null 2>&1; then command -v "$name"; return; fi
    if [ -x "$HOME/.cargo/bin/$name" ]; then printf '%s\n' "$HOME/.cargo/bin/$name"; return; fi
    echo "FAIL: $name not found on PATH or in ~/.cargo/bin; set \$$3" >&2
    exit 2
}
TXTODOD=$(resolve_bin txtodod "${TXTODOD:-}" TXTODOD)
TXTODO=$(resolve_bin txtodo "${TXTODO:-}" TXTODO)
echo "using $TXTODOD and $TXTODO" >&2
# The test-only in-memory keystore would make this check meaningless: never inherit it.
unset TXTODO_TEST_KEYSTORE_MEMORY
# Never let `txtodo doctor` spawn a daemon of its own if ours is not up yet.
export TXTODO_NO_AUTOSTART=1

# Short /tmp path: unix socket paths must stay under ~100 chars (sockaddr_un.sun_path).
# Ref: https://man7.org/linux/man-pages/man7/unix.7.html
WS=$(mktemp -d /tmp/txrk.XXXXXX)
printf 'relay id keychain probe\n' > "$WS/todo.txt"
PID=
# Ref: https://www.gnu.org/software/bash/manual/html_node/Bourne-Shell-Builtins.html#index-trap
cleanup() {
    if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
    rm -rf "$WS"
}
trap cleanup EXIT

# Starts one daemon, waits until doctor names a bound relay node id, stores it in LAST_ID, stops
# the daemon. Called directly, never in $(...): a subshell would hide PID from the EXIT trap and
# leave the daemon running on a failure or Ctrl-C.
LAST_ID=
node_id_of_one_start() {
    local run=$1 id='' start=$SECONDS
    "$TXTODOD" --dir "$WS" --no-lan --relay http://127.0.0.1:9 --key-store os \
        >"$WS/daemon-$run.log" 2>&1 &
    PID=$!
    echo "run $run: txtodod pid $PID; waiting for the relay to bind (answer any keychain prompt)" >&2
    while [ -z "$id" ]; do
        if ! kill -0 "$PID" 2>/dev/null; then
            echo "run $run: txtodod exited early; its log:" >&2
            cat "$WS/daemon-$run.log" >&2
            return 1
        fi
        if [ $((SECONDS - start)) -ge "$TIMEOUT_S" ]; then
            echo "run $run: no relay node id after ${TIMEOUT_S}s (a keychain prompt left open?)" >&2
            return 1
        fi
        # doctor exits 1 on any failed check; only its relay line matters here.
        id=$("$TXTODO" doctor --dir "$WS" 2>/dev/null | grep -oE 'node id [0-9a-f]{64}' | cut -d' ' -f3 || true)
        [ -n "$id" ] || sleep 1
    done
    kill "$PID"; wait "$PID" 2>/dev/null || true; PID=
    echo "run $run: node id $id" >&2
    LAST_ID=$id
}

node_id_of_one_start 1 || { echo "FAIL: first start"; exit 1; }
first=$LAST_ID
node_id_of_one_start 2 || { echo "FAIL: second start"; exit 1; }
second=$LAST_ID

if [ "$first" = "$second" ]; then
    echo "PASS: relay node id is stable across a restart ($first)"
else
    echo "FAIL: relay node id changed across a restart ($first -> $second)"
    exit 1
fi
