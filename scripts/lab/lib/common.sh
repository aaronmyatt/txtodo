# scripts/lab/lib/common.sh — paths, logging and small helpers every lab file uses.
# Sourced by scripts/lab/lab.sh; not run on its own.

# The checkout this script lives in (a worktree or the main one).
LAB_ROOT=$(git -C "$LAB_SCRIPTS" rev-parse --show-toplevel)
LAB_DEPLOY="$LAB_ROOT/deploy/lab"
LAB_SCENARIOS="$LAB_SCRIPTS/scenarios"

# Reports, run logs, the lock and the results table. Outside every workspace on purpose: the
# daemon's walker does not read .gitignore, and a report holds copies of devices' todo.txt.
LAB_HOME=${TXTODO_LAB_HOME:-$HOME/.local/state/txtodo-lab}

# Throwaway passphrase for the devices' file keystores, fed to txtodod on stdin inside a
# container that `compose down -v` deletes. Never a real secret.
export LAB_KEY_PASSPHRASE=lab-only-throwaway-passphrase
# The daemons' TXTODO_LOG (EnvFilter syntax, crates/txtodo-telemetry/src/lib.rs).
export LAB_LOG=${LAB_LOG:-info}

# Scenario order for `all`: quick LAN ones first, then the ones that need the relay.
LAB_ALL_SCENARIOS=(concurrent-adds lan-converge bad-link sleep clock-skew chaos old-new relay-only lan-to-relay nat-holepunch)

# Prints a timestamped line to stdout (the run log) and to the scenario's steps.log if one is open.
log() {
  local line
  line="$(date +%H:%M:%S) $*"
  printf '%s\n' "$line"
  if [ -n "${STEP_LOG:-}" ]; then
    printf '%s\n' "$line" >>"$STEP_LOG"
  fi
}

die() {
  log "error: $*"
  exit 1
}

# The main checkout: the first entry of `git worktree list --porcelain`. Failures are filed into
# its todo.txt even when the lab runs from a worktree.
# https://git-scm.com/docs/git-worktree#_porcelain_format
lab_main_checkout() {
  git -C "$LAB_ROOT" worktree list --porcelain | sed -n '1s/^worktree //p'
}

# $HOME/... -> ~/... for paths written into todo.txt, which syncs to other devices.
tilde_path() {
  # shellcheck disable=SC2088 # a literal ~ for the reader, not for a shell to expand
  case "$1" in
    "$HOME"/*) printf '~/%s\n' "${1#"$HOME"/}" ;;
    *) printf '%s\n' "$1" ;;
  esac
}

# Waits until "$@" succeeds, polling every second, for at most $1 seconds. Returns 1 on timeout.
wait_for() {
  local secs=$1
  shift
  local deadline=$((SECONDS + secs))
  until "$@"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      return 1
    fi
    sleep 1
  done
}

# A macOS notification when a run ends; a no-op anywhere else or if it fails.
# https://ss64.com/mac/osascript.html
notify() {
  if [ "$(uname -s)" = Darwin ]; then
    osascript -e "display notification \"$1\" with title \"txtodo lab\"" >/dev/null 2>&1 || true
  fi
}
