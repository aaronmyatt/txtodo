# scripts/lab/scenarios/old-new.sh — sourced by scripts/lab/lib/run.sh.
# The newest older release whose crates/ differ, paired with this tree; the seed picks which side.
TOPOLOGY=lan
DEVICES=(a1 b1)

scenario_setup() {
  ensure_old_image || skip "no older release to pair with, or its image did not build"
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
