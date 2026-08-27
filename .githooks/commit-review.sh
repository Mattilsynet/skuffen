#!/usr/bin/env bash
# Parallel AI review of a staged change. Any language, any repository.
#
#   hard gates  mechanical, absolute, no discretion   -> refuse the commit
#   signals     mechanical observations               -> raise scrutiny only
#   review      orchestrator + parallel investigators -> ALLOW / WARN / BLOCK
#
# Usage: commit-review.sh
set -euo pipefail

POLICY_VERSION=2

REPO=$(git rev-parse --show-toplevel)
GITDIR=$(git rev-parse --git-dir)
case "$GITDIR" in /*) ;; *) GITDIR="$REPO/$GITDIR" ;; esac
HOOKDIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

say() { printf 'commit-review: %s\n' "$*" >&2; }
die() {
  say "$*"
  exit 1
}

if [ "${COMMIT_REVIEW_SKIP:-}" = "1" ]; then
  say "SKIPPED via COMMIT_REVIEW_SKIP=1 -- deliberate, unreviewed"
  exit 0
fi

STRICT="${COMMIT_REVIEW_STRICT:-0}"
TIMEOUT="${COMMIT_REVIEW_TIMEOUT:-420}"

TREE=$(git write-tree)
BASE=""
git rev-parse --verify -q HEAD >/dev/null 2>&1 && BASE="HEAD"

POLICY=$(cat "$HOOKDIR/commit-review.sh" "$HOOKDIR/signals.sh" 2>/dev/null | shasum -a 256 | cut -c1-16)
POLICY="${POLICY}v${POLICY_VERSION}"
RECEIPTS="$GITDIR/commit-review/receipts"
mkdir -p "$RECEIPTS"
RECEIPT="$RECEIPTS/$TREE.$POLICY"

if [ -f "$RECEIPT" ]; then
  say "already reviewed ($(cat "$RECEIPT")) -- tree $TREE"
  exit 0
fi

# ------------------------------------------------------------------- evidence

BUNDLE=$(mktemp -d "${TMPDIR:-/tmp}/commit-review.XXXXXXXX")
mkdir -p "$BUNDLE/tree" "$BUNDLE/lock" "$BUNDLE/depsrc"

git checkout-index -a -f --prefix="$BUNDLE/tree/"
git diff --cached >"$BUNDLE/diff.patch"
git diff --cached --stat >"$BUNDLE/diff.stat"
git diff --cached --numstat >"$BUNDLE/diff.numstat"
git diff --cached --raw >"$BUNDLE/diff.raw"
git diff --cached --name-only >"$BUNDLE/files-changed.txt"

cat >"$BUNDLE/meta.json" <<EOF
{ "repo": "$REPO", "tree": "$TREE", "base": "${BASE:-}", "policy": "$POLICY",
  "generated": "$(date -u +%Y-%m-%dT%H:%M:%SZ)" }
EOF

# ----------------------------------------------------- dependency delta (any)
#
# Emits name@version lines for the lockfile formats we can parse. Formats we
# cannot are recorded in unparsed.txt so the reviewers know to read the raw
# diff rather than assuming nothing changed.

parse_lock() {
  local f="$1" kind="$2"
  [ -s "$f" ] || return 0
  case "$kind" in
  toml) # Cargo.lock, poetry.lock, uv.lock all use [[package]] name/version
    awk '/^\[\[package\]\]/{n="";next}
         /^name = /{n=$3;gsub(/"/,"",n);next}
         /^version = /{if(n!=""){v=$3;gsub(/"/,"",v);print n"@"v;n=""}}' "$f"
    ;;
  npm)
    jq -r '(.packages // {}) | to_entries[]
           | select(.key != "" and (.value.version // "") != "")
           | "\(.key | sub("^node_modules/";""))@\(.value.version)"' "$f" 2>/dev/null
    ;;
  gosum)
    awk '{sub(/\/go\.mod$/,"",$2); print $1"@"$2}' "$f"
    ;;
  reqs)
    grep -oE '^[A-Za-z0-9_.-]+==[^ ;]+' "$f" 2>/dev/null | tr '=' '@' | sed 's/@@/@/'
    ;;
  esac
}

: >"$BUNDLE/lock/.before"
: >"$BUNDLE/lock/.after"
: >"$BUNDLE/lock/unparsed.txt"

add_lock() {
  local path="$1" kind="$2"
  [ -f "$BUNDLE/tree/$path" ] || return 0
  if [ -n "$BASE" ]; then
    git show "$BASE:$path" >"$BUNDLE/lock/.b" 2>/dev/null || : >"$BUNDLE/lock/.b"
  else : >"$BUNDLE/lock/.b"; fi
  parse_lock "$BUNDLE/lock/.b" "$kind" >>"$BUNDLE/lock/.before" || true
  parse_lock "$BUNDLE/tree/$path" "$kind" >>"$BUNDLE/lock/.after" || true
}

add_lock Cargo.lock toml
add_lock poetry.lock toml
add_lock uv.lock toml
add_lock package-lock.json npm
add_lock go.sum gosum
add_lock requirements.txt reqs

for u in yarn.lock pnpm-lock.yaml bun.lock bun.lockb Gemfile.lock composer.lock; do
  if [ -f "$BUNDLE/tree/$u" ] && grep -qxF "$u" "$BUNDLE/files-changed.txt" 2>/dev/null; then
    echo "$u" >>"$BUNDLE/lock/unparsed.txt"
  fi
done

sort -u "$BUNDLE/lock/.before" -o "$BUNDLE/lock/.before"
sort -u "$BUNDLE/lock/.after" -o "$BUNDLE/lock/.after"
comm -13 "$BUNDLE/lock/.before" "$BUNDLE/lock/.after" >"$BUNDLE/lock/added.txt"
comm -23 "$BUNDLE/lock/.before" "$BUNDLE/lock/.after" >"$BUNDLE/lock/removed.txt"
cut -d@ -f1 "$BUNDLE/lock/added.txt" | sort -u >"$BUNDLE/lock/.an"
cut -d@ -f1 "$BUNDLE/lock/removed.txt" | sort -u >"$BUNDLE/lock/.rn"
comm -12 "$BUNDLE/lock/.an" "$BUNDLE/lock/.rn" >"$BUNDLE/lock/changed.txt"

# Copy source of newly added packages so investigators can read it with no
# network and no registry read root.
if [ -s "$BUNDLE/lock/added.txt" ]; then
  CARGO_SRC="${CARGO_HOME:-$HOME/.cargo}/registry/src"
  while IFS= read -r pkg; do
    [ -z "$pkg" ] && continue
    name="${pkg%@*}"
    ver="${pkg##*@}"
    for d in "$CARGO_SRC"/*/"$name-$ver" "$REPO/node_modules/$name"; do
      [ -d "$d" ] || continue
      sz=$(du -sk "$d" 2>/dev/null | cut -f1)
      dest="$BUNDLE/depsrc/$(printf '%s' "$name" | tr / _)-$ver"
      if [ "${sz:-0}" -gt 3072 ]; then
        mkdir -p "$dest"
        for keep in Cargo.toml package.json build.rs; do
          cp "$d/$keep" "$dest/" 2>/dev/null || true
        done
        echo "source truncated (${sz}KB); full copy at $d" >"$dest/TRUNCATED"
      else
        cp -R "$d" "$dest" 2>/dev/null || true
      fi
      break
    done
  done <"$BUNDLE/lock/added.txt"
fi

"$HOOKDIR/signals.sh" "$BUNDLE" "$REPO" "$BASE" || say "signal collection failed (non-fatal)"

if [ "${COMMIT_REVIEW_DRY:-}" = "1" ]; then
  say "dry run -- bundle built, no gates, no review"
  printf '\n--- bundle: %s\n--- files: %s   added: %s   removed: %s   depsrc: %s\n--- signals:\n' \
    "$BUNDLE" \
    "$(wc -l <"$BUNDLE/files-changed.txt" | tr -d ' ')" \
    "$(wc -l <"$BUNDLE/lock/added.txt" | tr -d ' ')" \
    "$(wc -l <"$BUNDLE/lock/removed.txt" | tr -d ' ')" \
    "$(find "$BUNDLE/depsrc" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')"
  sed 's/^/      /' "$BUNDLE/signals.txt"
  exit 0
fi

# ----------------------------------------------------------------- hard gates
#
# Absolutes only. Anything a legitimate change could ever trip belongs in
# signals instead. A tool that is not installed is reported as skipped, never
# treated as a pass.

MECH="$BUNDLE/mechanical.txt"
: >"$MECH"
FAILED=0

run_gate() {
  local label="$1"
  shift
  local log
  log=$(mktemp "$BUNDLE/.gate.XXXXXX") # never inherit the OS temp dir: it is
  if "$@" >"$log" 2>&1; then           # unreadable under the sandbox
    printf '[pass] %s\n' "$label" >>"$MECH"
  else
    printf '[FAIL] %s\n' "$label" >>"$MECH"
    sed 's/^/    /' "$log" >>"$MECH"
    FAILED=1
  fi
  rm -f "$log"
}
skip_gate() { printf '[skip] %s\n' "$1" >>"$MECH"; }

if command -v gitleaks >/dev/null 2>&1; then
  run_gate "gitleaks (staged snapshot)" \
    gitleaks detect --no-git --source "$BUNDLE/tree" --no-banner --redact
else
  skip_gate "gitleaks not installed"
fi

if [ -f "$BUNDLE/tree/Cargo.toml" ]; then
  run_gate "Cargo manifest parses" bash -c \
    "cd '$BUNDLE/tree' && cargo metadata --no-deps --format-version 1 >/dev/null"
  if [ -s "$BUNDLE/tree/Cargo.lock" ]; then
    if cargo audit --version >/dev/null 2>&1; then
      run_gate "cargo audit" bash -c "cd '$BUNDLE/tree' && cargo audit"
    else skip_gate "cargo-audit not installed"; fi
    if command -v cargo-deny >/dev/null 2>&1 && [ -f "$BUNDLE/tree/deny.toml" ]; then
      run_gate "cargo deny" bash -c "cd '$BUNDLE/tree' && cargo deny check advisories sources"
    else skip_gate "cargo-deny not configured"; fi
  fi
fi

if [ -f "$BUNDLE/tree/package.json" ]; then
  run_gate "package.json parses" jq -e . "$BUNDLE/tree/package.json"
fi
for j in $(grep -E '\.json$' "$BUNDLE/files-changed.txt" 2>/dev/null || true); do
  [ -f "$BUNDLE/tree/$j" ] && run_gate "$j parses" jq -e . "$BUNDLE/tree/$j"
done

if [ "$FAILED" -ne 0 ]; then
  say "hard gate failed -- absolute, not subject to review"
  grep -E '^\[FAIL\]' "$MECH" >&2 || true
  say "detail: $MECH"
  exit 1
fi

# --------------------------------------------------------------------- review

if ! command -v opencode >/dev/null 2>&1; then
  say "opencode not found; hard gates passed, skipping review"
  [ "$STRICT" = "1" ] && die "COMMIT_REVIEW_STRICT=1 and no opencode available"
  exit 0
fi

# Depth is the cost driver, not width: a slice re-sends a growing context every
# step. Small changes get fewer lenses so the common commit stays quick.
CHANGED_N=$(wc -l <"$BUNDLE/files-changed.txt" | tr -d ' ')
LINES_N=$(grep -cE '^[-+]' "$BUNDLE/diff.patch" 2>/dev/null || echo 0)
if [ -n "${COMMIT_REVIEW_LENSES:-}" ]; then
  LENSES="$COMMIT_REVIEW_LENSES"
elif [ -s "$BUNDLE/lock/added.txt" ] || [ -s "$BUNDLE/lock/unparsed.txt" ]; then
  LENSES=4
elif [ "$CHANGED_N" -le 3 ] && [ "$LINES_N" -le 120 ]; then
  LENSES=2
else
  LENSES=3
fi

if [ -n "${COMMIT_REVIEW_SLICE_MODEL:-}" ]; then
  export OPENCODE_CONFIG_CONTENT
  # Built with jq, not printf: the value is operator-supplied and lands inside
  # a JSON string that opencode parses.
  OPENCODE_CONFIG_CONTENT=$(jq -cn --arg m "$COMMIT_REVIEW_SLICE_MODEL" \
    '{agent:{CommitSlice:{model:$m},ProvenanceSlice:{model:$m}}}')
  say "slice model overridden to $COMMIT_REVIEW_SLICE_MODEL"
fi

with_timeout() {
  local secs="$1"
  shift
  "$@" &
  local pid=$!
  (
    sleep "$secs"
    kill -TERM "$pid" 2>/dev/null
    sleep 5
    kill -KILL "$pid" 2>/dev/null
  ) >/dev/null 2>&1 &
  local watch=$!
  local rc=0
  wait "$pid" || rc=$?
  kill "$watch" 2>/dev/null || true
  return "$rc"
}

# The survey is inlined rather than left for the agent to fetch. Sequential
# Read calls at the top of a session establish a one-tool-per-message rhythm
# that carries into the dispatch and makes the fan-out run serially -- measured
# twice. Handing the survey over makes the batch dispatch the first action.
SURVEY=$(
  printf 'tree=%s  files changed=%s  diff lines=%s\n\n' "$TREE" "$CHANGED_N" "$LINES_N"
  printf -- '--- diff stat ---\n'
  tail -25 "$BUNDLE/diff.stat"
  printf -- '\n--- signals (scrutiny hints, NOT failures) ---\n'
  cat "$BUNDLE/signals.txt"
  printf -- '\n--- packages added (%s) ---\n' "$(wc -l <"$BUNDLE/lock/added.txt" | tr -d ' ')"
  head -40 "$BUNDLE/lock/added.txt"
  printf -- '\n--- files changed ---\n'
  head -60 "$BUNDLE/files-changed.txt"
)

PROMPT="Review this staged change. The survey is below; you do not need to read
it again.

$SURVEY

Evidence bundle: $BUNDLE
  diff.patch  full diff          tree/    full post-change snapshot
  depsrc/     new dependency source        lock/  added.txt removed.txt changed.txt
  mechanical.txt  hard gates that already passed

YOUR FIRST ACTION IS THE FAN-OUT. Emit ONE message containing MULTIPLE task
tool calls -- about ${LENSES} of them, one per review lens this change most
needs. Do not dispatch one and wait. Do not read files first. Serial dispatch
overruns the ${TIMEOUT}s timeout and the review is lost.

CommitSlice reads code, history and depsrc. ProvenanceSlice answers registry,
publisher and upstream-repository questions. Give each a general lens and this
change as its starting point, not a boundary.

When they return, print a short human-readable report and then the verdict JSON
between <<<COMMIT_REVIEW_VERDICT_JSON and >>>END_COMMIT_REVIEW_VERDICT_JSON.
Do not dispatch a second wave. Do not write any file."

say "reviewing tree $TREE -- ${LENSES} lenses, ${TIMEOUT}s cap -- bundle $BUNDLE"

# The investigators run with the repository as their working directory so they
# can read real history. That also puts the live git index within reach of a
# stray `git add`. Point the subprocess at a throwaway copy instead.
cp "${GIT_INDEX_FILE:-$GITDIR/index}" "$BUNDLE/index.scratch" 2>/dev/null || true

set +e
with_timeout "$TIMEOUT" \
  env GIT_INDEX_FILE="$BUNDLE/index.scratch" \
  opencode run --pure --agent CommitReview "$PROMPT" 2>&1 | tee "$BUNDLE/review.log"
RC=${PIPESTATUS[0]}
set -e
case "$RC" in 124 | 137 | 143) say "review exceeded ${TIMEOUT}s and was killed" ;; esac

# ---------------------------------------------------------- extract & enforce
#
# stdout is the only verdict channel: it needs no permission and cannot be
# denied. Take the LAST sentinel block in case the agent revised itself.

VERDICT="$BUNDLE/verdict.json"
LC_ALL=C sed 's/\x1b\[[0-9;]*m//g' "$BUNDLE/review.log" 2>/dev/null |
  awk '/<<<COMMIT_REVIEW_VERDICT_JSON/{buf="";inblk=1;next}
       />>>END_COMMIT_REVIEW_VERDICT_JSON/{inblk=0;last=buf;next}
       inblk{buf=buf $0 "\n"}
       END{printf "%s", last}' >"$VERDICT" 2>/dev/null || true

if [ ! -s "$VERDICT" ] || ! jq -e . "$VERDICT" >/dev/null 2>&1; then
  say "review produced no usable verdict (opencode exit $RC)"
  [ "$STRICT" = "1" ] && die "COMMIT_REVIEW_STRICT=1 -- refusing without a verdict"
  say "hard gates passed; allowing. Set COMMIT_REVIEW_STRICT=1 to refuse instead."
  exit 0
fi

DECISION=$(jq -r '.decision // "MISSING"' "$VERDICT")

# Every added package must be accounted for. Omission fails closed, so being
# vague or skipping entries cannot buy a pass.
jq -r '.dependencies_added[]?.package' "$VERDICT" 2>/dev/null | sort -u >"$BUNDLE/lock/.acct"
comm -23 "$BUNDLE/lock/added.txt" "$BUNDLE/lock/.acct" >"$BUNDLE/lock/.unacct"
UNJUST=$(jq -r '[.dependencies_added[]? | select(.assessment == "unjustified")] | length' "$VERDICT")
OVERRIDE=""
if [ -s "$BUNDLE/lock/.unacct" ]; then
  OVERRIDE="packages added but unaccounted for: $(tr '\n' ' ' <"$BUNDLE/lock/.unacct")"
  DECISION=BLOCK
elif [ "${UNJUST:-0}" -gt 0 ]; then
  OVERRIDE="$UNJUST added package(s) assessed as unjustified"
  DECISION=BLOCK
fi

# Rendering is cosmetic relative to the decision; it must never be able to
# turn a WARN into a failed commit.
"$HOOKDIR/report.sh" "$VERDICT" "$DECISION" "$BUNDLE" "$OVERRIDE" ||
  say "report rendering failed; decision was $DECISION (verdict: $VERDICT)"

case "$DECISION" in
ALLOW | WARN)
  printf '%s %s %s\n' "$DECISION" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$BUNDLE" >"$RECEIPT"
  exit 0
  ;;
BLOCK)
  exit 1
  ;;
*)
  say "unrecognised decision '$DECISION'"
  [ "$STRICT" = "1" ] && die "COMMIT_REVIEW_STRICT=1 -- refusing"
  exit 0
  ;;
esac
