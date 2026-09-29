# scripts/lab/lib/faults.sh — what the scenarios do to devices: cut and restore links, kill and
# restart daemons, freeze them, make links bad, move their clocks. Sourced by scripts/lab/lab.sh.
# Every fault that heal_all should undo is written to $REPORT_DIR/state/faults.

fault_note() {
  printf '%s\n' "$*" >>"$REPORT_DIR/state/faults"
}

# unplug <device> <network>: the device leaves that network (cable pulled, Wi-Fi gone).
# https://docs.docker.com/reference/cli/docker/network/disconnect/
unplug() {
  docker network disconnect -f "${PROJECT}_$2" "$(cname "$1")"
  log "fault: $1 unplugged from $2"
}

# plug <device> <network> [ip]: back on. Without an ip, Docker hands out a new address, like
# DHCP would. The routes toward a NAT router are put back, since reconnecting may reset them.
# https://docs.docker.com/reference/cli/docker/network/connect/
plug() {
  local dev=$1 net=$2 ip=${3:-}
  if [ -n "$ip" ]; then
    docker network connect --ip "$ip" "${PROJECT}_$net" "$(cname "$dev")"
  else
    docker network connect "${PROJECT}_$net" "$(cname "$dev")"
  fi
  reroute "$dev"
  log "fault: $dev plugged back into $net${ip:+ at $ip}"
}

# The same /24 and default routes the entrypoint sets for a device behind a router.
reroute() {
  dx "$1" sh -c '[ -z "${LAB_GATEWAY:-}" ] || { ip route replace 10.231.0.0/24 via "$LAB_GATEWAY" &&
    ip route replace default via "$LAB_GATEWAY"; }' || true
}

# partition <device> <network> [ip]: unplug now, heal_all plugs it back.
partition() {
  unplug "$1" "$2"
  fault_note "net $1 $2 ${3:-}"
}

# crash <device>: SIGKILL the daemon's container, no clean shutdown.
# https://docs.docker.com/reference/cli/docker/container/kill/
crash() {
  docker kill -s KILL "$(cname "$1")" >/dev/null
  fault_note "down $1"
  log "fault: $1 killed (SIGKILL)"
}

# restart <device>: start it again and wait for its daemon.
restart() {
  docker start "$(cname "$1")" >/dev/null
  log "fault: $1 started again"
  wait_ready "$1" 90
}

# freeze / thaw <device>: every process in the container stops (a laptop lid closed).
# https://docs.docker.com/reference/cli/docker/container/pause/
freeze() {
  docker pause "$(cname "$1")" >/dev/null
  fault_note "paused $1"
  log "fault: $1 paused"
}

thaw() {
  docker unpause "$(cname "$1")" >/dev/null 2>&1 || true
  log "fault: $1 unpaused"
}

# The container's interfaces other than loopback.
ifaces_of() {
  dx "$1" sh -c "ip -o link show | awk -F': ' '{ print \$2 }' | cut -d@ -f1 | grep -vx lo"
}

# badlink <device> <netem args...>: delay, loss, reorder, duplicates on every interface.
# Returns 1 if the kernel has no netem. https://man7.org/linux/man-pages/man8/tc-netem.8.html
badlink() {
  local dev=$1 i
  shift
  for i in $(ifaces_of "$dev"); do
    dx "$dev" tc qdisc replace dev "$i" root netem "$@" || return 1
  done
  fault_note "netem $dev"
  log "fault: $dev link now: $*"
}

goodlink() {
  local dev=$1 i
  for i in $(ifaces_of "$dev"); do
    dx "$dev" tc qdisc del dev "$i" root >/dev/null 2>&1 || true
  done
}

# set_clock <device> <offset>: libfaketime's offset for that device, e.g. "+7m", "-10m", "+0".
# Takes effect within a second (FAKETIME_CACHE_DURATION), for the daemon and the device's CLI,
# but only on a device whose LD_PRELOAD loads libfaketime (LAB_PRELOAD_<DEV>).
# https://github.com/wolfcw/libfaketime#readme
set_clock() {
  dx "$1" sh -c "printf '%s\n' '$2' >/lab/.faketime"
  if [ "$2" != "+0" ]; then
    fault_note "clock $1"
  fi
  log "fault: $1 clock offset $2 (now $(dx "$1" date -u +%H:%M:%S) there, $(date -u +%H:%M:%S) here)"
}

on_network() {
  docker inspect -f '{{json .NetworkSettings.Networks}}' "$(cname "$1")" |
    jq -e --arg n "${PROJECT}_$2" 'has($n)' >/dev/null
}

# heal_all <device...>: undo every recorded fault, so the final checks see a healthy network.
# A device that is down without a recorded `crash` exited on its own: that is a failure (its
# daemon died), and it is started again so the other checks still run.
heal_all() {
  local kind dev net ip killed=" "
  touch "$REPORT_DIR/state/faults"
  # The list is read on fd 3: any command in the loop that reads stdin (a `docker exec -i` did)
  # would otherwise eat the rest of it and end the loop after the first fault.
  # https://www.gnu.org/software/bash/manual/bash.html#Redirections
  while read -r kind dev net ip <&3; do
    case "$kind" in
      down) killed+="$dev " ;;
      paused) if is_paused "$dev"; then thaw "$dev"; fi ;;
      netem) goodlink "$dev" ;;
      clock) set_clock "$dev" +0 ;;
      net) if ! on_network "$dev" "$net"; then plug "$dev" "$net" "$ip"; fi ;;
    esac
  done 3<"$REPORT_DIR/state/faults"
  for dev in "$@"; do
    if is_paused "$dev"; then
      thaw "$dev"
    fi
    if ! is_running "$dev"; then
      if [[ "$killed" != *" $dev "* ]]; then
        fail "daemon: $dev exited on its own (exit code $(docker inspect -f '{{.State.ExitCode}}' "$(cname "$dev")"))"
      fi
      restart "$dev" || true
    fi
  done
  : >"$REPORT_DIR/state/faults"
}
