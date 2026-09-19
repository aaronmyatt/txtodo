#!/usr/bin/env bash
# Cold-boot timing for tasks/daemon-early-bind: stop the daemon, start it, print the milliseconds
# until it first answers and until each registered workspace is Ready, and fail when the first
# answer takes longer than the limit (default 2000 ms). A daemon that binds its socket first
# answers in milliseconds however many workspaces it still has to open.
#
# Two modes:
#   real device daemon (default): `txtodo daemon stop`, then `txtodo daemon start` — restarts the
#     launchd/systemd unit, so run it when you can afford one cold boot.
#   isolated: with TXTODO_SOCKET and TXTODO_REGISTRY_DB both set (a throwaway registry, as the test
#     suites use), spawns `${TXTODOD:-txtodod}` itself against them and stops it afterwards; the
#     real daemon and the real registry are never touched.
#
# Usage: scripts/cold-boot-timing.sh [--limit-ms N] [--ready-timeout-s N]
# Needs: txtodo on PATH (or $TXTODO), jq, perl.
# Ref: https://jqlang.org/manual/ · https://perldoc.perl.org/Time::HiRes
set -euo pipefail

LIMIT_MS=2000
READY_TIMEOUT_S=600
while [ $# -gt 0 ]; do
    case "$1" in
        --limit-ms) LIMIT_MS=$2; shift 2 ;;
        --ready-timeout-s) READY_TIMEOUT_S=$2; shift 2 ;;
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

TXTODO=${TXTODO:-txtodo}
TXTODOD=${TXTODOD:-txtodod}
# Polls must never start a daemon of their own: that would hide the boot being measured.
export TXTODO_NO_AUTOSTART=1

now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time() * 1000'; }

isolated=0
if [ -n "${TXTODO_SOCKET:-}" ] && [ -n "${TXTODO_REGISTRY_DB:-}" ]; then
    isolated=1
fi

daemon_pid=""
seen=$(mktemp)
cleanup() {
    rm -f "$seen"
    [ -z "$daemon_pid" ] || kill "$daemon_pid" 2>/dev/null || true
}
trap cleanup EXIT

stop_daemon() {
    if [ "$isolated" = 1 ]; then
        [ -z "$daemon_pid" ] || kill "$daemon_pid" 2>/dev/null || true
        [ -z "$daemon_pid" ] || wait "$daemon_pid" 2>/dev/null || true
        daemon_pid=""
    else
        "$TXTODO" daemon stop >/dev/null 2>&1 || true
    fi
}

# `txtodo workspace list --json`: one JSON object per line, each with `id`, `root` and `load_state`.
list_json() { "$TXTODO" --json workspace list 2>/dev/null; }

echo "cold-boot-timing: stopping the daemon"
stop_daemon
sleep 1

start=$(now_ms)
if [ "$isolated" = 1 ]; then
    "$TXTODOD" --no-lan --no-relay >/dev/null 2>&1 &
    daemon_pid=$!
else
    env -u TXTODO_NO_AUTOSTART "$TXTODO" daemon start >/dev/null 2>&1 &
fi

first=""
snapshot=""
while [ -z "$first" ]; do
    if snapshot=$(list_json); then
        first=$(( $(now_ms) - start ))
        break
    fi
    [ $(( $(now_ms) - start )) -lt $(( READY_TIMEOUT_S * 1000 )) ] || { echo "cold-boot-timing: FAIL -- no answer within ${READY_TIMEOUT_S}s" >&2; exit 1; }
    sleep 0.05
done
echo "first answer: ${first} ms"

# One line per settled workspace in $seen: "<ms>\t<state>\t<id>\t<root>" (no associative arrays:
# macOS ships bash 3.2).
deadline=$(( start + READY_TIMEOUT_S * 1000 ))
while :; do
    all_settled=1
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        id=$(jq -r '.id' <<<"$line")
        state=$(jq -r '.load_state // "unknown"' <<<"$line")
        root=$(jq -r '.root' <<<"$line")
        case "$state" in
            ready|failed)
                if ! grep -q "	$id	" "$seen"; then
                    printf '%s\t%s\t%s\t%s\n' "$(( $(now_ms) - start ))" "$state" "$id" "$root" >>"$seen"
                fi ;;
            *) all_settled=0 ;;
        esac
    done <<<"$snapshot"
    [ "$all_settled" = 1 ] && break
    [ "$(now_ms)" -lt "$deadline" ] || { echo "cold-boot-timing: some workspaces never settled within ${READY_TIMEOUT_S}s" >&2; break; }
    sleep 0.5
    snapshot=$(list_json) || true
done

sort -n "$seen" | while IFS=$'\t' read -r ms state id root; do echo "${state}: ${ms} ms  ${root}"; done

[ "$isolated" = 1 ] && stop_daemon
if [ "$first" -gt "$LIMIT_MS" ]; then
    echo "cold-boot-timing: FAIL -- first answer took ${first} ms, limit ${LIMIT_MS} ms" >&2
    exit 1
fi
echo "cold-boot-timing: ok -- first answer ${first} ms (limit ${LIMIT_MS} ms)"
