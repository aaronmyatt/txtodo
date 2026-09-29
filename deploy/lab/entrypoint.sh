#!/usr/bin/env bash
# deploy/lab/entrypoint.sh — what every P2P lab container runs under compose's `init: true`
# (https://docs.docker.com/reference/compose-file/services/#init). LAB_ROLE picks the part:
#   device  txtodod on this container's own network stack (the default)
#   router  a NAT gateway between a "home" network and the lab "internet"
#   relay   the iroh relay, plain HTTP on :3340
set -euo pipefail

# Every lab address is in 10.231.0.0/16 (deploy/lab/compose.*.yml).
LAB_NET=10.231.0.0/16

# The lab's networks are plain bridges, not `internal: true`: Docker's isolation rules for an
# internal network drop any packet whose destination is outside that network's subnet, which
# kills mDNS multicast and everything a NAT router forwards. So each container seals itself:
# only loopback, lab addresses and multicast may leave it, never the real internet.
# https://man7.org/linux/man-pages/man8/iptables.8.html
seal_egress() {
  iptables -A OUTPUT -o lo -j ACCEPT
  iptables -A OUTPUT -d "$LAB_NET" -j ACCEPT
  iptables -A OUTPUT -d 224.0.0.0/4 -j ACCEPT
  iptables -A OUTPUT -j DROP
}

role_router() {
  seal_egress
  # Forward only toward the lab internet (and the replies conntrack matches back): a home device
  # can reach the relay and the other router's WAN address, never the other home directly and
  # never outside the lab. https://man7.org/linux/man-pages/man8/iptables-extensions.8.html (conntrack)
  iptables -A FORWARD -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
  iptables -A FORWARD -d "${LAB_WAN_PREFIX}0/24" -j ACCEPT
  iptables -A FORWARD -j DROP
  # The outside interface is the one holding an address that starts with LAB_WAN_PREFIX.
  # `ip -o` prints one line per address: https://man7.org/linux/man-pages/man8/ip-address.8.html
  local wan
  wan=$(ip -o -4 addr show | awk -v p="$LAB_WAN_PREFIX" 'index($4, p) == 1 { print $2; exit }')
  if [ -z "$wan" ]; then
    echo "router: no interface with an address in ${LAB_WAN_PREFIX}*" >&2
    exit 1
  fi
  # MASQUERADE rewrites a home device's source address to this router's WAN address.
  # --random-fully gives every flow a fresh random source port: a "hard" NAT that defeats hole
  # punching, so only the relay path is left.
  # https://man7.org/linux/man-pages/man8/iptables-extensions.8.html (MASQUERADE)
  if [ "${LAB_NAT:-easy}" = hard ]; then
    iptables -t nat -A POSTROUTING -o "$wan" -j MASQUERADE --random-fully
  else
    iptables -t nat -A POSTROUTING -o "$wan" -j MASQUERADE
  fi
  echo "router: ${LAB_NAT:-easy} NAT out of $wan"
  exec sleep infinity
}

role_relay() {
  seal_egress
  exec iroh-relay --dev --config-path /etc/iroh-relay.toml
}

role_device() {
  # A restarted container gets a fresh network namespace, so this runs on every start; the check
  # only guards against sealing twice.
  if ! iptables -C OUTPUT -j DROP 2>/dev/null; then
    seal_egress
  fi
  # Behind a NAT router: the lab internet is reached through it. The /24 route is more specific
  # than any default route Docker sets when a scenario reconnects the LAN, so it survives that.
  # https://man7.org/linux/man-pages/man8/ip-route.8.html
  if [ -n "${LAB_GATEWAY:-}" ]; then
    ip route replace 10.231.0.0/24 via "$LAB_GATEWAY"
    ip route replace default via "$LAB_GATEWAY"
  fi
  # libfaketime reads this file when LD_PRELOAD loads it (clock-skew scenario only); "+0" = real time.
  if [ ! -f /lab/.faketime ]; then
    echo "+0" >/lab/.faketime
  fi
  # No keychain in a container: without a flag the daemon falls back to memory and turns sync off.
  # `--key-store file` keeps identity across `docker kill`. The passphrase is read from stdin only;
  # LAB_KEY_PASSPHRASE is a throwaway lab string, never a real secret. Process substitution makes
  # txtodod this shell's direct replacement, so signals reach it:
  # https://www.gnu.org/software/bash/manual/bash.html#Process-Substitution
  local args=(--key-store file)
  if [ "${LAB_RELAY:-off}" = off ]; then
    args+=(--no-relay)
  else
    args+=(--relay "$LAB_RELAY")
  fi
  if [ "${LAB_LAN:-on}" = off ]; then
    args+=(--no-lan)
  fi
  echo "device: txtodod ${args[*]}"
  exec txtodod "${args[@]}" < <(printf '%s\n' "$LAB_KEY_PASSPHRASE")
}

case "${LAB_ROLE:-device}" in
  router) role_router ;;
  relay) role_relay ;;
  device) role_device ;;
  *)
    echo "entrypoint: unknown LAB_ROLE=${LAB_ROLE}" >&2
    exit 2
    ;;
esac
