# scripts/lab/lib/workload.sh — seeded random edits on devices, and the bookkeeping the no-loss
# check needs. Sourced by scripts/lab/lab.sh.
#
# Every edit carries a unique token (t<seed><device>r<round>n<i>). A token lands in one of three
# lists under $REPORT_DIR/state/:
#   added    a line or paragraph with it was written; it must survive on every device...
#   touched  ...unless a delete, replace or editor rewrite (or an editor save that overwrote
#            lines that had just arrived) removed the line that held it;
#   soft     written onto an existing line (append, replace): a concurrent delete of that line
#            may take it legitimately, so it is not checked.
# `txtodo move` is left out: through the daemon it runs on a temp copy holding only todo.txt.

LAB_WORDS=(apple brick cedar delta ember fjord grove harbor island jasper kettle lemon meadow
  nickel orbit pepper quartz river saddle timber umber velvet willow yarrow zephyr)

token_regex() {
  printf 't%s[a-z][0-9]r[0-9]+n[0-9]+' "$SEED"
}

# Tokens found in stdin, one per line. These filters end in `|| true`: under `set -o pipefail` a
# grep that matches nothing would otherwise fail the whole pipeline, and `set -e` the run.
tokens_in() {
  grep -oE "$(token_regex)" || true
}

# stdin minus the token $1.
without() {
  grep -vxF "$1" || true
}

# Only the CLOBBER lines of an editor save's output.
clobbered() {
  grep '^CLOBBER ' || true
}

# record <list> < tokens
record() {
  cat >>"$REPORT_DIR/state/$1"
}

ops_log() {
  printf '%s %s\n' "$(date +%H:%M:%S)" "$*" >>"$REPORT_DIR/ops.log"
}

# Candidate line numbers in a device's root todo.txt: all non-blank ones, or only open ones.
lines_of() {
  local dev=$1 which=${2:-any}
  if [ "$which" = open ]; then
    dx "$dev" awk 'NF && $1 != "x" { print NR }' "$WS/todo.txt" 2>/dev/null
  else
    dx "$dev" awk 'NF { print NR }' "$WS/todo.txt" 2>/dev/null
  fi
}

# Sets LINE to a random entry of the candidates (uses the caller's RANDOM stream), or returns 1.
pick_line() {
  local candidates
  mapfile -t candidates < <(lines_of "$1" "${2:-any}")
  if [ "${#candidates[@]}" -eq 0 ]; then
    return 1
  fi
  LINE=${candidates[RANDOM % ${#candidates[@]}]}
}

# Sets TEXT to "<token> <two words> +lab @<device>".
make_text() {
  local tok=$1 dev=$2
  TEXT="$tok ${LAB_WORDS[RANDOM % ${#LAB_WORDS[@]}]} ${LAB_WORDS[RANDOM % ${#LAB_WORDS[@]}]} +lab @$dev"
}

# An editor-style save, run inside the device in one `sh -c` so the window is tiny: copy the
# file, write the new version to a temp file, copy the file again, rename the temp file over it
# (https://man7.org/linux/man-pages/man2/rename.2.html). Prints "OLD <line>" for a rewritten
# line and "CLOBBER <line>" for every line that arrived between the two copies: the rename
# overwrote those, the way a real editor save would.
# Args: <file under the workspace> <append|edit> <new line> [line number for edit]
EDITOR_SAVE_SH='
f="$WS/$1"; mode=$2; text=$3; n=${4:-0}
mkdir -p "$(dirname "$f")"; [ -f "$f" ] || : >"$f"
cp "$f" /tmp/ed.before
if [ "$mode" = append ]; then
  { cat /tmp/ed.before
    if [ -s /tmp/ed.before ] && [ -n "$(tail -c1 /tmp/ed.before)" ]; then echo; fi
    printf "%s\n" "$text"; } >"$f.labtmp"
else
  awk -v n="$n" -v t="$text" "NR == n { print \"OLD \" \$0 > \"/dev/stderr\"; print t; next } { print }" \
    /tmp/ed.before >"$f.labtmp"
fi
cp "$f" /tmp/ed.again
mv "$f.labtmp" "$f"
grep -Fxv -f /tmp/ed.before /tmp/ed.again | grep . | sed "s/^/CLOBBER /"
exit 0
'

editor_save() {
  local dev=$1
  shift
  docker exec -i -e WS="$WS" "$(cname "$dev")" sh -c "$EDITOR_SAVE_SH" editor "$@" 2>&1
}

# One random edit on one device. Needs RANDOM already seeded by the caller.
random_op() {
  local dev=$1 tok=$2 roll out rc=0 file
  roll=$((RANDOM % 100))
  make_text "$tok" "$dev"
  if [ "$roll" -lt 30 ]; then
    out=$(tx "$dev" add "$TEXT") || rc=$?
    [ "$rc" -eq 0 ] && printf '%s\n' "$tok" | record added
    ops_log "$dev add $tok rc=$rc"
  elif [ "$roll" -lt 40 ]; then
    pick_line "$dev" open || return 0
    out=$(tx "$dev" "do" "$LINE") || rc=$?
    ops_log "$dev do $LINE rc=$rc"
  elif [ "$roll" -lt 47 ]; then
    pick_line "$dev" open || return 0
    local pris=(A B C)
    out=$(tx "$dev" pri "$LINE" "${pris[RANDOM % 3]}") || rc=$?
    ops_log "$dev pri $LINE rc=$rc"
  elif [ "$roll" -lt 54 ]; then
    pick_line "$dev" || return 0
    out=$(tx "$dev" append "$LINE" "and $tok") || rc=$?
    [ "$rc" -eq 0 ] && printf '%s\n' "$tok" | record soft
    ops_log "$dev append $LINE $tok rc=$rc"
  elif [ "$roll" -lt 61 ]; then
    pick_line "$dev" || return 0
    out=$(tx "$dev" replace "$LINE" "$TEXT") || rc=$?
    # replace prints the old line, then the new one: the old one's tokens are gone.
    printf '%s\n' "$out" | tokens_in | without "$tok" | record touched
    printf '%s\n' "$tok" | record soft
    ops_log "$dev replace $LINE $tok rc=$rc"
  elif [ "$roll" -lt 68 ]; then
    pick_line "$dev" || return 0
    out=$(tx "$dev" del "$LINE") || rc=$?
    # del prints the line it deleted.
    printf '%s\n' "$out" | tokens_in | record touched
    ops_log "$dev del $LINE rc=$rc"
  elif [ "$roll" -lt 70 ]; then
    out=$(tx "$dev" archive) || rc=$?
    ops_log "$dev archive rc=$rc"
  elif [ "$roll" -lt 82 ]; then
    out=$(editor_save "$dev" todo.txt append "$(date +%F) $TEXT") || rc=$?
    [ "$rc" -eq 0 ] && printf '%s\n' "$tok" | record added
    printf '%s\n' "$out" | clobbered | tokens_in | record touched
    ops_log "$dev editor-append todo.txt $tok rc=$rc"
  elif [ "$roll" -lt 88 ]; then
    pick_line "$dev" || return 0
    out=$(editor_save "$dev" todo.txt edit "$(date +%F) $TEXT" "$LINE") || rc=$?
    printf '%s\n' "$out" | tokens_in | without "$tok" | record touched
    printf '%s\n' "$tok" | record soft
    ops_log "$dev editor-edit todo.txt $LINE $tok rc=$rc"
  else
    if [ $((RANDOM % 2)) -eq 0 ]; then
      file=tasks/lab/todo.txt
      out=$(editor_save "$dev" "$file" append "$(date +%F) $TEXT") || rc=$?
    else
      file=tasks/lab/notes.md
      out=$(editor_save "$dev" "$file" append "- $TEXT") || rc=$?
    fi
    [ "$rc" -eq 0 ] && printf '%s\n' "$tok" | record added
    printf '%s\n' "$out" | clobbered | tokens_in | record touched
    ops_log "$dev editor-append $file $tok rc=$rc"
  fi
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$dev" >>"$REPORT_DIR/state/op-errors"
  fi
  return 0
}

# workload <round> <ops per device> <device...>: every running, unpaused device edits at the same
# time, each from its own RANDOM stream (a subshell's RANDOM would otherwise repeat the
# parent's). Short random gaps keep the edits interleaved with incoming sync.
workload() {
  local round=$1 n=$2 dev idx=0 pids=()
  shift 2
  mkdir -p "$REPORT_DIR/state"
  for dev in "$@"; do
    idx=$((idx + 1))
    if ! is_running "$dev" || is_paused "$dev"; then
      continue
    fi
    (
      RANDOM=$((SEED * 1000 + round * 10 + idx))
      local i
      for ((i = 1; i <= n; i++)); do
        random_op "$dev" "t${SEED}${dev}r${round}n${i}"
        sleep "0.$((RANDOM % 4))"
      done
    ) &
    pids+=($!)
  done
  if [ "${#pids[@]}" -gt 0 ]; then
    wait "${pids[@]}" || true
  fi
  log "workload: round $round done ($n ops on each of: $*)"
}
