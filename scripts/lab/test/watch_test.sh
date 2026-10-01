#!/usr/bin/env bash
# scripts/lab/test/watch_test.sh — the background worker's logic (scripts/lab/lib/watch.sh) with a
# fake runner and a scratch repo: no Docker, a few seconds. Run it after changing the worker:
#   scripts/lab/test/watch_test.sh
# A scratch repo gets eight commits; scenario s-flip fails while crates/x.txt says "bad" (commits
# 4 to 6). The worker must: take only the newest of several queued commits, skip one that
# touches nothing the lab builds, bisect the regression to commit 4, file one line naming it, and
# report the fix at commit 7.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'git -C "$tmp/repo" worktree remove --force "$tmp/wt" 2>/dev/null || true; rm -rf "$tmp"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

git init --quiet -b main "$tmp/repo"
commit() { # commit <file> <text> <message>
  mkdir -p "$(dirname "$tmp/repo/$1")"
  printf '%s\n' "$2" >"$tmp/repo/$1"
  git -C "$tmp/repo" add -A
  git -C "$tmp/repo" -c user.name=lab -c user.email=lab@example.invalid commit --quiet -m "$3"
  git -C "$tmp/repo" rev-parse HEAD
}
c1=$(commit crates/x.txt good "c1 good")
c2=$(commit crates/y.txt one "c2")
c3=$(commit crates/y.txt two "c3")
c4=$(commit crates/x.txt bad "c4 breaks s-flip")
c5=$(commit README.md docs "c5 docs only")
c6=$(commit crates/y.txt three "c6")
c7=$(commit crates/x.txt good "c7 fixes s-flip")
c8=$(commit crates/y.txt four "c8")

# The runner contract (lib/watch.sh): `<runner> <scenarios> <seed>`, LAB_SOURCE and LAB_RUN_ID
# set, rows appended to $TXTODO_LAB_HOME/results.tsv.
cat >"$tmp/runner.sh" <<'RUNNER'
#!/usr/bin/env bash
set -euo pipefail
scenarios=$1
[ "$scenarios" = all ] && scenarios=s-good,s-flip
IFS=, read -r -a list <<<"$scenarios"
for s in "${list[@]}"; do
  v=pass
  if [ "$s" = s-flip ] && grep -q bad "$LAB_SOURCE/crates/x.txt"; then v=fail; fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$(date '+%F %T')" "$LAB_RUN_ID" "$s" "$2" "$v" \
    "/reports/$LAB_RUN_ID-$s.md" >>"$TXTODO_LAB_HOME/results.tsv"
done
RUNNER
chmod +x "$tmp/runner.sh"
mkdir -p "$tmp/backlog" "$tmp/home"
: >"$tmp/backlog/todo.txt"

export TXTODO_LAB_HOME=$tmp/home LAB_WATCH_REPO=$tmp/repo LAB_WATCH_WORKTREE=$tmp/wt \
  LAB_WATCH_RUNNER=$tmp/runner.sh LAB_WATCH_POLL=1 LAB_NO_NOTIFY=1 \
  LAB_TASK_NO_DAEMON=1 LAB_BACKLOG_DIR=$tmp/backlog TXTODO_CONFIG=$tmp/none.toml
watch() { "$here/watch.sh" "$@" 2>>"$tmp/watch.log"; }
verdicts=$tmp/home/watch/verdicts.tsv

watch queue "$c1"
watch once
grep -q "$c1	full	.*	s-flip	pass" "$verdicts" || fail "c1 is the baseline"

watch queue "$c5"
[ ! -s "$tmp/home/watch/queue" ] || fail "a docs-only commit is not queued"
for c in "$c2" "$c3" "$c6"; do watch queue "$c"; done
watch once
grep -q "$c6	full" "$verdicts" || fail "the newest queued commit is tested"
! grep -q "$c3	full" "$verdicts" || fail "older queued commits are dropped"
grep -q "$c6	confirm	.*	s-flip	fail" "$verdicts" || fail "a regression is re-run once"
grep -q "regressed at ${c4:0:8}" "$tmp/watch.log" || fail "bisect finds c4: $(cat "$tmp/watch.log")"
grep -q "lab: s-flip regressed, seed 1072683562: at ${c4:0:8} c4 breaks s-flip" "$tmp/backlog/todo.txt" ||
  fail "one line filed: $(cat "$tmp/backlog/todo.txt")"
! grep -q "s-good.*regressed" "$tmp/watch.log" || fail "a scenario that still passes is quiet"

watch queue "$c7"
watch once
grep -q "s-flip passes from ${c7:0:8} on" "$tmp/watch.log" || fail "the fix is reported"

watch queue "$c8"
watch queue "$c8"
watch once
[ "$(grep -c "$c8	full	.*	s-flip" "$verdicts")" = 1 ] || fail "a commit queued twice is tested once"

echo "watch_test: ok"
