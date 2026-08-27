#!/usr/bin/env bash
# Render a verdict deterministically.
#
# The reader is usually another agent -- the one that ran `git commit` -- and
# sometimes a human. Both are served by the same fixed structure, which is why
# this rendering happens here rather than being left to the reviewing model's
# prose. Same shape every time: parseable without heuristics, scannable without
# reading.
#
# Usage: report.sh <verdict.json> <decision> <bundle> [override-reason]
set -uo pipefail

V="$1"
D="$2"
B="$3"
OVERRIDE="${4:-}"

rule() { printf '%s\n' "────────────────────────────────────────────────────────────"; }

printf '\n'
rule
printf 'COMMIT REVIEW: %s\n' "$D"
rule

jq -r '.summary // "(no summary)"' "$V" | fold -s -w 76

if [ -n "$OVERRIDE" ]; then
  printf '\nDECISION OVERRIDDEN BY THE HOOK\n  %s\n' "$OVERRIDE"
  printf '  Unaccounted or unjustified dependencies fail closed, regardless of\n'
  printf '  what the review concluded.\n'
fi

DEPS=$(jq -r '.dependencies_added // [] | length' "$V")
if [ "$DEPS" -gt 0 ]; then
  printf '\nDEPENDENCIES ADDED (%s)\n' "$DEPS"
  jq -r '.dependencies_added[]? |
    "  [\(.assessment // "?")] \(.package // "?")\n      \(.justification // "")"' "$V" |
    fold -s -w 76
fi

FN=$(jq -r '.findings // [] | length' "$V")
if [ "$FN" -gt 0 ]; then
  printf '\nFINDINGS (%s)\n' "$FN"
  jq -r '.findings // [] |
    sort_by((.severity // "info") as $s
            | ["critical","high","medium","low","info"] | index($s) // 99) | .[] |
    "  [\((.severity // "info") | ascii_upcase)] \(.title // "")\n      lens: \(.lens // "?")\n      \(.evidence // "")\n      -> \(.recommendation // "")"' "$V" |
    fold -s -w 76
fi

UN=$(jq -r '.unresolved // [] | length' "$V")
if [ "$UN" -gt 0 ]; then
  printf '\nUNRESOLVED (%s)\n' "$UN"
  jq -r '.unresolved[]? | "  - \(.)"' "$V" | fold -s -w 76
fi

printf '\nCOVERAGE: %s lens(es)' "$(jq -r '.coverage.lenses_dispatched // "?"' "$V")"
GAPS=$(jq -r '.coverage.gaps // [] | length' "$V")
[ "$GAPS" -gt 0 ] && printf ', %s gap(s)' "$GAPS"
printf '\n'
jq -r '.coverage.gaps[]? | "  - \(.)"' "$V" | fold -s -w 76

rule
printf 'FOR THE AGENT READING THIS\n'
case "$D" in
ALLOW)
  cat <<'EOF'
  The commit was created. No material concern was found.
  Continue with the task. There is nothing here the human needs told.
EOF
  ;;
WARN)
  cat <<'EOF'
  The commit WAS created. Concerns were found and judged proportionate.

  You must surface the findings above to the human in your next message --
  briefly, in your own words, highest severity first. Do not silently move on
  because the commit succeeded; WARN means "proceeded, and you should know".

  Do not re-run the commit and do not use --no-verify. If a finding looks
  wrong, say so and let the human decide.
EOF
  ;;
BLOCK)
  cat <<'EOF'
  The commit was REFUSED. Nothing was committed.

  Fix the cause and commit again, or tell the human why you believe the
  finding is mistaken. Both are legitimate; routing around the gate is not.

  Do NOT use --no-verify, -n, COMMIT_REVIEW_SKIP=1, or core.hooksPath=/dev/null.
  Do NOT unstage the flagged file merely to get the commit through. A blocked
  commit is a question to answer, not an obstacle to clear.
EOF
  ;;
*)
  printf '  Unrecognised decision. Treat as unreviewed and tell the human.\n'
  ;;
esac
printf '\n  evidence: %s\n' "$B"
rule
printf '\n'
