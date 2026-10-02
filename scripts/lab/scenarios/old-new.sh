# scripts/lab/scenarios/old-new.sh — sourced by scripts/lab/lib/run.sh.
# The newest older release whose crates/ differ, paired with this tree; the seed picks which side.
TOPOLOGY=lan
DEVICES=(a1 b1)

# The sync protocol a ref speaks (`PROTOCOL_VERSION` in txtodo-sync's frame.rs).
protocol_at() {
  git -C "$LAB_ROOT" show "$1:crates/txtodo-sync/src/frame.rs" 2>/dev/null |
    sed -n 's/^pub const PROTOCOL_VERSION: u16 = \([0-9]*\);/\1/p'
}

scenario_setup() {
  ensure_old_image || skip "no older release to pair with, or its image did not build"
  # ADR 0035: devices on different sync protocols refuse each other, pairing included, so there
  # is nothing to converge until a release speaks this tree's protocol.
  local old new
  old=$(protocol_at "$LAB_OLD_REF_USED")
  new=$(protocol_at HEAD)
  if [ -n "$old" ] && [ "$old" != "$new" ]; then
    skip "old release $LAB_OLD_REF_USED speaks sync protocol $old, this tree $new: they do not pair (ADR 0035)"
  fi
  if [ $((SEED % 2)) -eq 0 ]; then
    export LAB_IMAGE_A1=$LAB_OLD_IMAGE
    OLD=a1
  else
    export LAB_IMAGE_B1=$LAB_OLD_IMAGE
    OLD=b1
  fi
  note "old release $LAB_OLD_REF_USED ($LAB_OLD_IMAGE) runs on $OLD; a1 offers the pairing"
}

scenario_main() {
  pair a1 b1 || return 1
  workload 1 6 a1 b1
  expect_converged "old and new" 120 a1 b1

  partition b1 lan
  workload 2 6 a1 b1
  heal_all a1 b1
  expect_converged "partition healed" 120 a1 b1

  crash a1
  workload 3 4 b1
  restart a1
  workload 4 6 a1 b1
}
