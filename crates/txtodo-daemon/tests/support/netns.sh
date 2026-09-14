#!/usr/bin/env bash
# tests/support/netns.sh — plan M8 relay-converge-test, todo.txt item 1: two Linux network
# namespaces (A, B) reachable only through a third (relay), no shared NIC, no path between A and B
# except via the relay's veth links. This is the real network-boundary topology
# `crates/txtodo-daemon/tests/relay_converge.rs` is written against on Linux CI; that same test
# suite runs on this development sandbox (macOS, no root, no `ip netns` at all — Linux network
# namespaces do not exist on this OS) as two real OS processes on one host instead, which is a
# different and weaker guarantee — see that test file's own module doc for exactly what it could
# and could not prove here, and `RELAY_CONVERGE_CI.patch.md` for wiring this script into CI.
#
# Requires: Linux, `ip` (iproute2), CAP_NET_ADMIN (root or `sudo`). Written and reviewed, but never
# executed in this session — this sandbox has neither Linux nor root. A human running this on real
# Linux CI should treat the first run as the actual verification; `probe` alone is cheap and safe
# to run repeatedly.
#
# Topology:
#   netns A --- veth-a-relay <-> veth-relay-a --- netns relay --- veth-relay-b <-> veth-b-relay --- netns B
#   A:  10.99.1.2/30  <-> relay: 10.99.1.1/30   (link 1)
#   B:  10.99.2.2/30  <-> relay: 10.99.2.1/30   (link 2)
# A has no route to 10.99.2.0/30 and B has no route to 10.99.1.0/30 — neither namespace's default
# route exists at all (no `ip netns exec nsA ip route add default ...`), so the only way traffic
# from A reaches B is by being addressed to the relay (which forwards application-level frames —
# ciphertext blobs or a QUIC relay's own hole-punch/forwarding role — never IP-forwards between the
# two subnets: `sysctl net.ipv4.ip_forward` is left at its namespace default of 0 throughout).
#
# Usage:
#   sudo tests/support/netns.sh up      # create the three namespaces and veth links
#   sudo tests/support/netns.sh probe   # assert A cannot reach B directly (see "probe" below)
#   sudo tests/support/netns.sh down    # tear everything down (idempotent, safe if already gone)
#   sudo tests/support/netns.sh exec <a|b|relay> <command...>   # run a command inside one netns
set -euo pipefail

NS_A="txtodo-relay-test-a"
NS_B="txtodo-relay-test-b"
NS_RELAY="txtodo-relay-test-relay"

VETH_A="veth-a-relay"
VETH_A_PEER="veth-relay-a"
VETH_B="veth-b-relay"
VETH_B_PEER="veth-relay-b"

IP_A="10.99.1.2/30"
IP_A_RELAY_SIDE="10.99.1.1/30"
IP_B="10.99.2.2/30"
IP_B_RELAY_SIDE="10.99.2.1/30"
IP_A_HOST="10.99.1.1"
IP_B_HOST="10.99.2.1"

require_root() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "netns.sh: must run as root (CAP_NET_ADMIN) — try: sudo $0 $*" >&2
    exit 1
  fi
}

require_linux() {
  if [ "$(uname -s)" != "Linux" ]; then
    echo "netns.sh: Linux network namespaces do not exist on $(uname -s) — this script only runs on Linux CI." >&2
    exit 1
  fi
  command -v ip >/dev/null 2>&1 || {
    echo "netns.sh: 'ip' (iproute2) not found." >&2
    exit 1
  }
}

# Deletes a namespace if it exists; never fails if it doesn't (idempotent, so `down` after a failed
# or partial `up` cannot itself fail the CI job).
delete_ns() {
  ip netns del "$1" 2>/dev/null || true
}

cmd_down() {
  delete_ns "$NS_A"
  delete_ns "$NS_B"
  delete_ns "$NS_RELAY"
  # Any leftover veth on the host side (a veth pair's other end is deleted automatically when its
  # sibling's namespace is deleted, but the host-side end of a pair that never made it into a
  # namespace — an interrupted `up` — has to be cleaned up explicitly).
  ip link del "$VETH_A" 2>/dev/null || true
  ip link del "$VETH_B" 2>/dev/null || true
}

# One veth pair, one end into `ns`, both ends addressed and brought up. `host_dev`/`ns_dev` are the
# link names; `host_ip`/`ns_ip` are CIDR addresses for the relay-side (host-created) and the
# namespace-side ends respectively.
link_into_ns() {
  local ns="$1" host_dev="$2" ns_dev="$3" ns_ip="$4" host_ip="$5"
  ip link add "$host_dev" type veth peer name "$ns_dev"
  ip link set "$ns_dev" netns "$ns"
  ip addr add "$host_ip" dev "$host_dev"
  ip link set "$host_dev" up
  ip netns exec "$ns" ip addr add "$ns_ip" dev "$ns_dev"
  ip netns exec "$ns" ip link set "$ns_dev" up
  ip netns exec "$ns" ip link set lo up
}

cmd_up() {
  require_linux
  require_root up
  cmd_down # idempotent: never build on top of a stale namespace from a previous, interrupted run
  ip netns add "$NS_A"
  ip netns add "$NS_B"
  ip netns add "$NS_RELAY"
  # The relay-side ends (VETH_A_PEER/VETH_B_PEER) live in the relay namespace, addressed there;
  # the host-created ends (VETH_A/VETH_B) are moved straight into A/B respectively — so every leg
  # of both links has exactly one namespace on each side, never the host's own default namespace.
  ip link add "$VETH_A" type veth peer name "$VETH_A_PEER"
  ip link set "$VETH_A" netns "$NS_A"
  ip link set "$VETH_A_PEER" netns "$NS_RELAY"
  ip netns exec "$NS_A" ip addr add "$IP_A" dev "$VETH_A"
  ip netns exec "$NS_A" ip link set "$VETH_A" up
  ip netns exec "$NS_A" ip link set lo up
  ip netns exec "$NS_RELAY" ip addr add "$IP_A_RELAY_SIDE" dev "$VETH_A_PEER"
  ip netns exec "$NS_RELAY" ip link set "$VETH_A_PEER" up

  ip link add "$VETH_B" type veth peer name "$VETH_B_PEER"
  ip link set "$VETH_B" netns "$NS_B"
  ip link set "$VETH_B_PEER" netns "$NS_RELAY"
  ip netns exec "$NS_B" ip addr add "$IP_B" dev "$VETH_B"
  ip netns exec "$NS_B" ip link set "$VETH_B" up
  ip netns exec "$NS_B" ip link set lo up
  ip netns exec "$NS_RELAY" ip addr add "$IP_B_RELAY_SIDE" dev "$VETH_B_PEER"
  ip netns exec "$NS_RELAY" ip link set "$VETH_B_PEER" up
  ip netns exec "$NS_RELAY" ip link set lo up

  # Deliberately no `ip route add default` in A or B, and no `ip_forward=1` in relay: the relay
  # namespace is a dead end at the IP layer for both subnets, exactly as the module doc describes.
  echo "netns.sh: up — A=$NS_A ($IP_A) B=$NS_B ($IP_B) relay=$NS_RELAY ($IP_A_RELAY_SIDE, $IP_B_RELAY_SIDE)"
}

# The boundary probe (todo.txt item 11): A must NOT be able to reach B's address directly. `nc -z`
# with a short timeout against a host that is simply unreachable at the IP layer (no route) fails
# fast; this asserts that failure, which is the whole point — success here would mean the topology
# leaked a path relay_converge.rs's convergence proof would then wrongly credit to the relay.
cmd_probe() {
  require_linux
  require_root probe
  local out
  if out=$(ip netns exec "$NS_A" timeout 2 nc -z -w1 "$IP_B_HOST" 1 2>&1); then
    echo "netns.sh: PROBE FAILED — A reached the relay-side address of B's link ($IP_B_HOST); topology is not isolated: $out" >&2
    exit 1
  fi
  echo "netns.sh: PROBE OK — A cannot reach $IP_B_HOST (no route; expected: only the relay is reachable from either side)"
}

cmd_exec() {
  require_linux
  require_root exec
  local which="$1"
  shift
  local ns
  case "$which" in
    a) ns="$NS_A" ;;
    b) ns="$NS_B" ;;
    relay) ns="$NS_RELAY" ;;
    *)
      echo "netns.sh: exec's first argument must be a, b or relay, got $which" >&2
      exit 1
      ;;
  esac
  exec ip netns exec "$ns" "$@"
}

case "${1:-}" in
  up) cmd_up ;;
  down) cmd_down ;;
  probe) cmd_probe ;;
  exec)
    shift
    cmd_exec "$@"
    ;;
  *)
    echo "usage: $0 up|down|probe|exec <a|b|relay> <command...>" >&2
    exit 2
    ;;
esac
