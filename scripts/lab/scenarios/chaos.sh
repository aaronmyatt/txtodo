# scripts/lab/scenarios/chaos.sh — sourced by scripts/lab/lib/run.sh.
# Three devices on a LAN under a seeded loop of partitions, kills, pauses and bad links.
TOPOLOGY=lan
DEVICES=(a1 b1 a2)
PROFILE=three
CONVERGE_TIMEOUT=300

scenario_main() {
  pair a1 b1 || return 1
  pair a1 a2 || return 1
  workload 0 4 a1 b1 a2
  expect_converged "three paired" 120 a1 b1 a2

  # The seed picks each round's victim and fault; the timing is still the machine's.
  local round victim fault
  RANDOM=$SEED
  for round in 1 2 3 4 5 6 7 8; do
    victim=${DEVICES[RANDOM % 3]}
    fault=$((RANDOM % 5))
    case "$fault" in
      0) log "chaos: round $round, no fault" ;;
      1) partition "$victim" lan ;;
      2) crash "$victim" ;;
      3) freeze "$victim" ;;
      4) badlink "$victim" delay 100ms 50ms loss 10% reorder 20% 50% || true ;;
    esac
    workload "$round" 5 a1 b1 a2
    sleep $((RANDOM % 5))
    heal_all a1 b1 a2
  done
}
