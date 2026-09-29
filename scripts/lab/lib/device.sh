# scripts/lab/lib/device.sh — running commands on a lab device, waiting for its daemon, pairing.
# Sourced by scripts/lab/lab.sh. PROJECT (the compose project) is set per scenario.

# The default workspace every device's CLI falls back to (global mode, no --dir):
# <XDG data dir>/txtodo/default with HOME=/lab (crates/txtodo-workspace-paths).
WS=/lab/.local/share/txtodo/default
STATE=/lab/.local/share/txtodo

# Compose names containers <project>-<service>-<n>.
# https://docs.docker.com/compose/how-tos/project-name/
cname() {
  printf '%s-%s-1\n' "$PROJECT" "$1"
}

# dx <device> <command...>: run a command inside a device.
dx() {
  local dev=$1
  shift
  docker exec "$(cname "$dev")" "$@"
}

# tx <device> <txtodo args...>: the device's own CLI against its own daemon, bounded so a hung
# daemon fails the call instead of the run. `timeout`: https://man7.org/linux/man-pages/man1/timeout.1.html
tx() {
  local dev=$1
  shift
  docker exec "$(cname "$dev")" timeout "${TX_TIMEOUT:-60}" txtodo "$@" 2>/dev/null
}

is_running() {
  [ "$(docker inspect -f '{{.State.Running}}' "$(cname "$1")" 2>/dev/null)" = true ]
}

is_paused() {
  [ "$(docker inspect -f '{{.State.Paused}}' "$(cname "$1")" 2>/dev/null)" = true ]
}

daemon_answers() {
  dx "$1" timeout 5 txtodo daemon status >/dev/null 2>&1
}

# wait_ready <device> [secs]: until the daemon's socket answers. Records a failure on timeout.
wait_ready() {
  local dev=$1 secs=${2:-60}
  if wait_for "$secs" daemon_answers "$dev"; then
    return 0
  fi
  fail "daemon: $dev did not answer within ${secs}s"
  return 1
}

pair_code_ready() {
  dx "$1" grep -q "waiting for a device" /tmp/pair.out 2>/dev/null
}

pair_initiator_done() {
  dx "$1" grep -q "^pair-exit=" /tmp/pair.out 2>/dev/null
}

# pair <initiator> <joiner>: `txtodo pair` on one, `txtodo pair <code>` on the other, both
# answering yes to "do the six words match" and "is it your own device" (so the default
# workspaces merge). The initiator runs detached because it blocks until the joiner arrives.
# The code is read only once the line after it is out, so it is never read half-written.
pair() {
  local a=$1 b=$2 code out rc
  log "pair: $a offers, $b joins"
  dx "$a" rm -f /tmp/pair.out
  docker exec -d "$(cname "$a")" sh -c \
    'printf "y\ny\n" | timeout 170 txtodo pair >/tmp/pair.out 2>&1; echo "pair-exit=$?" >>/tmp/pair.out'
  if ! wait_for 30 pair_code_ready "$a"; then
    fail "pairing: $a printed no code within 30s"
    return 1
  fi
  code=$(dx "$a" sed -n '/other device):$/{n;p;q}' /tmp/pair.out | tr -d '[:space:]')
  rc=0
  out=$(printf 'y\ny\n' | docker exec -i "$(cname "$b")" timeout 170 txtodo pair "$code" 2>&1) || rc=$?
  printf '%s\n' "$out" >"$REPORT_DIR/pair-$a-$b.joiner.txt"
  wait_for 30 pair_initiator_done "$a" || true
  dx "$a" cat /tmp/pair.out >"$REPORT_DIR/pair-$a-$b.initiator.txt" 2>/dev/null || true
  if [ "$rc" -ne 0 ] || ! dx "$a" grep -q "^pair-exit=0$" /tmp/pair.out; then
    fail "pairing: $a + $b did not pair (joiner exit $rc; see pair-$a-$b.*.txt)"
    return 1
  fi
  log "pair: $a + $b paired"
}
