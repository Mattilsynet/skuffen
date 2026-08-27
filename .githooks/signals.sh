#!/usr/bin/env bash
# Mechanical observations for a proposed change, across any ecosystem.
#
# These are SENSORS, NOT POLICY. Nothing here fails a commit. Every line is a
# reason for the review agents to look harder, and each one is expected to have
# a good explanation most of the time. Turning any of these into an automatic
# failure is how the whole system gets bypassed.
#
# Usage: signals.sh <bundle-dir> <repo-root> <base-ref-or-empty>
set -euo pipefail

BUNDLE="$1"
REPO="$2"
BASE="${3:-}"
OUT="$BUNDLE/signals.txt"
: >"$OUT"

note() { printf '%s\n' "$*" >>"$OUT"; }
changed() { grep -qxF "$1" "$BUNDLE/files-changed.txt" 2>/dev/null; }
changed_re() { grep -qE "$1" "$BUNDLE/files-changed.txt" 2>/dev/null; }
before() { [ -n "$BASE" ] && git -C "$REPO" show "$BASE:$1" 2>/dev/null || true; }
after() { cat "$BUNDLE/tree/$1" 2>/dev/null || true; }
gained() { # gained <file> <pattern> -- present after, absent before
  after "$1" | grep -q "$2" 2>/dev/null && ! before "$1" | grep -q "$2" 2>/dev/null
}

# ------------------------------------------------------- build-time execution
#
# Code that runs at build or install time, in any language. This is the class
# the arrayref compromise used: the payload never appeared in the package that
# was audited, it ran from a dependency's build script.

while IFS= read -r f; do
  [ -z "$f" ] && continue
  case "$f" in
  build.rs | */build.rs)
    if [ -z "$(before "$f")" ]; then
      note "build-time: NEW Rust build script: $f"
    else
      note "build-time: Rust build script modified: $f"
    fi
    ;;
  *package.json)
    for s in preinstall install postinstall prepare prepublish; do
      gained "$f" "\"$s\"" && note "build-time: package.json gained a '$s' lifecycle script: $f"
    done
    ;;
  setup.py | */setup.py)
    [ -z "$(before "$f")" ] && note "build-time: NEW setup.py (executes on install): $f"
    ;;
  *.cargo/config.toml | *.cargo/config)
    note "build-time: cargo configuration changed: $f"
    ;;
  *Makefile | *makefile | *justfile | *Taskfile.yml)
    note "build-time: build automation changed: $f"
    ;;
  *Dockerfile | *.dockerfile)
    note "build-time: container build definition changed: $f"
    ;;
  *.pre-commit-config.yaml | *lefthook.yml | *.husky/*)
    note "build-time: git hook tooling changed: $f"
    ;;
  esac
done <"$BUNDLE/files-changed.txt"

while IFS= read -r f; do
  [ -z "$f" ] && continue
  case "$f" in
  *Cargo.toml)
    gained "$f" 'proc-macro *= *true' && note "build-time: crate became a proc-macro: $f"
    ;;
  esac
done <"$BUNDLE/files-changed.txt"

# ------------------------------------------------------------- gate weakening
#
# The review gate cannot police changes to itself.

if changed_re '(^|/)(deny\.toml|\.cargo/audit\.toml|\.gitleaks\.toml|gitleaks\.toml)$'; then
  note "gate: security tooling configuration changed"
fi
if changed_re '(^|/)(\.githooks|scripts/git-hooks)/'; then
  note "gate: git hook definitions changed"
fi

# Suppression lists that hide known-vulnerable dependencies.
for f in .cargo/audit.toml deny.toml .npmrc .snyk; do
  if changed "$f"; then
    B=$(before "$f" | grep -cE 'RUSTSEC-|CVE-|GHSA-' || true)
    A=$(after "$f" | grep -cE 'RUSTSEC-|CVE-|GHSA-' || true)
    if [ "${A:-0}" -gt "${B:-0}" ]; then
      note "gate: advisory suppression list in $f grew from ${B:-0} to ${A:-0}"
    fi
  fi
done

if grep -qE '^\+.*(RUSTFLAGS|NODE_OPTIONS|PYTHONPATH|LD_PRELOAD)' "$BUNDLE/diff.patch" 2>/dev/null; then
  note "gate: an environment variable that alters build or runtime behaviour was set"
fi

# ------------------------------------------------------------------- contents

if [ -f "$BUNDLE/diff.numstat" ]; then
  awk -F'\t' '$1=="-" && $2=="-" {print $3}' "$BUNDLE/diff.numstat" | while IFS= read -r f; do
    [ -n "$f" ] && note "contents: binary file in change (not reviewable as text): $f"
  done
fi

if [ -f "$BUNDLE/diff.raw" ]; then
  awk '$1 ~ /^:/ { old=substr($1,2); if (old!="000000" && old!=$2 && $2=="100755") { for(i=6;i<=NF;i++) printf "%s ", $i; print "" } }' \
    "$BUNDLE/diff.raw" | while IFS= read -r f; do
    [ -n "$f" ] && note "contents: file gained the executable bit:$f"
  done
fi

# --------------------------------------------------------------- dependencies

if [ -s "$BUNDLE/lock/added.txt" ]; then
  note "deps: $(wc -l <"$BUNDLE/lock/added.txt" | tr -d ' ') package(s) added"
fi
if [ -s "$BUNDLE/lock/removed.txt" ]; then
  note "deps: $(wc -l <"$BUNDLE/lock/removed.txt" | tr -d ' ') package(s) removed"
fi
if [ -s "$BUNDLE/lock/changed.txt" ]; then
  while IFS= read -r n; do
    [ -z "$n" ] && continue
    o=$(grep "^$n@" "$BUNDLE/lock/removed.txt" 2>/dev/null | head -1 | cut -d@ -f2-)
    w=$(grep "^$n@" "$BUNDLE/lock/added.txt" 2>/dev/null | head -1 | cut -d@ -f2-)
    note "deps: version change $n ${o:-?} -> ${w:-?}"
  done <"$BUNDLE/lock/changed.txt"
fi

# A lockfile that moved without its manifest moving means the change arrived
# transitively -- the shape a compromised transitive dependency produces.
if [ -s "$BUNDLE/lock/added.txt" ] &&
  ! changed_re '(^|/)(Cargo\.toml|package\.json|pyproject\.toml|go\.mod|requirements\.txt)$'; then
  note "deps: packages appeared without any manifest change (transitive)"
fi

if [ -f "$BUNDLE/lock/unparsed.txt" ]; then
  while IFS= read -r f; do
    [ -n "$f" ] && note "deps: $f changed but has no structured parser here; read the diff directly"
  done <"$BUNDLE/lock/unparsed.txt"
fi

if [ -d "$BUNDLE/depsrc" ]; then
  for d in "$BUNDLE"/depsrc/*/; do
    [ -d "$d" ] || continue
    name=$(basename "$d")
    [ -f "$d/build.rs" ] && note "deps: new dependency ships a build script: $name"
    if grep -q 'proc-macro *= *true' "$d/Cargo.toml" 2>/dev/null; then
      note "deps: new dependency is a proc-macro (runs at compile time): $name"
    fi
    if [ -f "$d/package.json" ]; then
      for s in preinstall install postinstall; do
        grep -q "\"$s\"" "$d/package.json" 2>/dev/null &&
          note "deps: new dependency has a '$s' script: $name"
      done
    fi
  done
fi

# Dependencies pulled straight from a git host float unless pinned.
if changed_re '(^|/)(Cargo\.toml|Cargo\.lock|package\.json|go\.mod)$'; then
  for m in Cargo.toml package.json; do
    [ -f "$BUNDLE/tree/$m" ] || continue
    { grep -nE 'git(\+ssh)? *[:=] *"?(https?|ssh|git)' "$BUNDLE/tree/$m" 2>/dev/null || true; } |
      while IFS= read -r line; do
        case "$line" in
        *tag[\ =]* | *rev[\ =]* | *branch[\ =]* | *\#[0-9a-f][0-9a-f][0-9a-f][0-9a-f]*) : ;;
        *) note "deps: unpinned git dependency in $m, and this change moves dependencies: ${line#*:}" ;;
        esac
      done
  done
fi

# ------------------------------------------------------------------------- ci
#
# Only when this change touches CI. Reporting standing repository conditions on
# every commit is how a signal list becomes wallpaper.

if changed_re '^\.github/workflows/'; then
  note "ci: workflow definitions changed"
  W="$BUNDLE/tree/.github/workflows"
  if [ -d "$W" ]; then
    { grep -rhoE 'uses: *"?[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[A-Za-z0-9_./-]+' "$W" 2>/dev/null || true; } |
      sed 's/^uses: *//; s/"//g' | sort -u | while IFS= read -r u; do
      printf '%s' "${u##*@}" | grep -qE '^[0-9a-f]{40}$' && continue
      note "ci: action on a mutable ref (repointable by its owner): $u"
    done
    { grep -rhoE 'pull_request_target|workflow_run' "$W" 2>/dev/null || true; } |
      sort -u | while IFS= read -r t; do
      [ -n "$t" ] && note "ci: privileged trigger present: $t"
    done
    { grep -rhoE '(secrets|vars)\.[A-Z_][A-Z0-9_]*' "$W" 2>/dev/null || true; } |
      sort -u | while IFS= read -r s; do
      [ -n "$s" ] && note "ci: workflow references $s"
    done
  fi
fi

if changed_re '^\.github/(CODEOWNERS|dependabot\.yml)$|^\.gitlab-ci\.yml$|^Jenkinsfile$'; then
  note "ci: repository governance or pipeline definition changed"
fi

[ -s "$OUT" ] || note "(no mechanical signals raised)"
exit 0
