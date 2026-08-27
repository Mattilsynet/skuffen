#!/usr/bin/env bash
# Run the bounded AI security review for the staged tree.
set -euo pipefail

TIMEOUT=${COMMIT_REVIEW_TIMEOUT:-180}
LENSES=${COMMIT_REVIEW_LENSES:-3}
REPO=$(git rev-parse --show-toplevel)
GITDIR=$(git rev-parse --git-dir)
case "$GITDIR" in /*) ;; *) GITDIR="$REPO/$GITDIR" ;; esac

say() { printf 'security-review: %s\n' "$*" >&2; }

BUNDLE=$(mktemp -d "${TMPDIR:-/tmp}/security-review.XXXXXXXX")
KEEP=${COMMIT_REVIEW_VERBOSE:-0}
cleanup() {
  if [ "$KEEP" = 1 ]; then
    say "evidence retained at $BUNDLE"
  else
    rm -rf "$BUNDLE"
  fi
}
trap cleanup EXIT

mkdir "$BUNDLE/tree"
say "preparing staged snapshot"
git checkout-index -a -f --prefix="$BUNDLE/tree/"
git diff --cached >"$BUNDLE/diff.patch"
git diff --cached --stat >"$BUNDLE/diff.stat"
git diff --cached --name-only >"$BUNDLE/files.txt"

FILES=$(wc -l <"$BUNDLE/files.txt" | tr -d ' ')
CHANGES=$(git diff --cached --shortstat | sed 's/^ *//')
say "snapshot ready — $FILES files, ${CHANGES:-no textual changes}"

if [ "${COMMIT_REVIEW_VERBOSE:-0}" = 1 ]; then
  say "evidence: $BUNDLE"
  sed 's/^/  /' "$BUNDLE/diff.stat" >&2
  sed 's/^/  /' "$BUNDLE/files.txt" >&2
fi

# Investigators can read repository history while Git mutations target this
# disposable index.
cp "${GIT_INDEX_FILE:-$GITDIR/index}" "$BUNDLE/index" 2>/dev/null || true

SURVEY="$(cat "$BUNDLE/diff.stat")

Changed paths:
$(cat "$BUNDLE/files.txt")"

PROMPT="Review the staged change for concrete security risk.

Use this survey to select exactly $LENSES complementary security lenses:

$SURVEY

Evidence bundle: $BUNDLE
- diff.patch: exact staged diff
- diff.stat: change summary
- files.txt: changed paths
- tree/: complete staged tree

Your first tool action is one assistant message containing all $LENSES task
calls. This starts every investigator concurrently. Use CommitSlice for local
static analysis and Sifter for external evidence. After this one wave, print the
verdict JSON between the required sentinels."

LOG="$BUNDLE/review.log"
FIFO="$BUNDLE/stdout"
mkfifo "$FIFO"
tee "$LOG" <"$FIFO" &
TEE_PID=$!

START=$(date +%s)
say "starting $LENSES parallel investigators — ${TIMEOUT}s budget"
env GIT_INDEX_FILE="$BUNDLE/index" \
  opencode run --pure --agent CommitReview "$PROMPT" >"$FIFO" 2>&1 &
PID=$!
(sleep "$TIMEOUT"; kill -TERM "$PID" 2>/dev/null; sleep 5; kill -KILL "$PID" 2>/dev/null) >/dev/null 2>&1 &
WATCH=$!

set +e
wait "$PID"
RC=$?
set -e
kill "$WATCH" 2>/dev/null || true
wait "$TEE_PID" || true
ELAPSED=$(($(date +%s) - START))

[ "$RC" -eq 0 ] || {
  KEEP=1
  say "review unavailable after ${ELAPSED}s (exit $RC); commit continues"
  exit 0
}

say "investigators and synthesis completed in ${ELAPSED}s"

VERDICT="$BUNDLE/verdict.json"
LC_ALL=C sed 's/\x1b\[[0-9;]*m//g' "$LOG" |
  awk '/<<<COMMIT_REVIEW_VERDICT_JSON/{buf="";inside=1;next}
       />>>END_COMMIT_REVIEW_VERDICT_JSON/{inside=0;last=buf;next}
       inside{buf=buf $0 "\n"}
       END{printf "%s",last}' >"$VERDICT"

if ! command -v jq >/dev/null 2>&1 || ! jq -e . "$VERDICT" >/dev/null 2>&1; then
  KEEP=1
  say "usable verdict unavailable; commit continues"
  exit 0
fi

DECISION=$(jq -r '.decision // ""' "$VERDICT")
say "$DECISION — $(jq -r '.summary // ""' "$VERDICT")"

case "$DECISION" in WARN | BLOCK) KEEP=1 ;; esac

[ "$DECISION" != BLOCK ]
