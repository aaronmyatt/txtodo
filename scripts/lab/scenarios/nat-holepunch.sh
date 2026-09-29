# scripts/lab/scenarios/nat-holepunch.sh — sourced by scripts/lab/lib/run.sh.
# Two homes behind easy NATs, no LAN: sync must work; whether a direct path forms is noted.
TOPOLOGY=world
DEVICES=(a1 b1)
CONVERGE_TIMEOUT=300

scenario_setup() {
  export LAB_NAT_A=easy LAB_NAT_B=easy
}

# The last connection-path lines a device logged (TXTODO_CONN_PATH_POLL=1 in compose.world.yml,
# crates/txtodo-sync/src/holepunch.rs). Only a note: the dev relay has no QUIC address discovery
# (it needs TLS), so a direct path is not expected yet.
note_path() {
  local line
  line=$(docker logs "$(cname "$1")" 2>&1 | sed $'s/\x1b\\[[0-9;]*m//g' |
    grep -E 'relay_connect_(established|path_poll)' | tail -1 | grep -oE 'path=[^ ]+( [^ ]+)?' || true)
  note "path: $1's last relay connection: ${line:-none logged}"
}

scenario_main() {
  unplug a1 lan
  unplug b1 lan
  pair a1 b1 || return 1
  workload 1 6 a1 b1
  expect_converged "behind two NATs" 180 a1 b1
  workload 2 6 a1 b1
  sleep 10
  note_path a1
  note_path b1
}
