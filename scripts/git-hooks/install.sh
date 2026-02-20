#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOKS_DIR="${REPO_ROOT}/.git/hooks"

mkdir -p "${HOOKS_DIR}"
cp "${REPO_ROOT}/scripts/git-hooks/pre-push" "${HOOKS_DIR}/pre-push"
chmod +x "${HOOKS_DIR}/pre-push"

install_gitleaks() {
  if command -v gitleaks >/dev/null 2>&1; then
    return 0
  fi
  if command -v brew >/dev/null 2>&1; then
    brew install gitleaks
    return 0
  fi
  echo "gitleaks not found. Please install manually: https://github.com/gitleaks/gitleaks" >&2
  return 1
}

install_gitleaks || true

echo "Installed pre-push hook. Update scripts/git-hooks/forbidden-patterns.txt as needed."
