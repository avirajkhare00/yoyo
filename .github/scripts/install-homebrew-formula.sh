#!/usr/bin/env bash
set -euo pipefail

formula_path="${1:?usage: install-homebrew-formula.sh <formula-path> [formula-name] [tap-name]}"
formula_name="${2:-yoyo}"
tap_name="${3:-yoyo/local-ci}"
git_author_name="${GIT_AUTHOR_NAME:-GitHub Actions}"
git_author_email="${GIT_AUTHOR_EMAIL:-github-actions[bot]@users.noreply.github.com}"

if [[ ! -f "$formula_path" ]]; then
  echo "Formula file not found: $formula_path" >&2
  exit 1
fi

export GIT_AUTHOR_NAME="$git_author_name"
export GIT_AUTHOR_EMAIL="$git_author_email"
export GIT_COMMITTER_NAME="${GIT_COMMITTER_NAME:-$GIT_AUTHOR_NAME}"
export GIT_COMMITTER_EMAIL="${GIT_COMMITTER_EMAIL:-$GIT_AUTHOR_EMAIL}"

brew uninstall --force "$formula_name" >/dev/null 2>&1 || true
brew untap "$tap_name" >/dev/null 2>&1 || true
brew tap-new "$tap_name"

tap_repo="$(brew --repository "$tap_name")"
mkdir -p "$tap_repo/Formula"
cp "$formula_path" "$tap_repo/Formula/${formula_name}.rb"

brew install "$tap_name/$formula_name"
"$(brew --prefix)/bin/${formula_name}" --version
brew test "$tap_name/$formula_name"
