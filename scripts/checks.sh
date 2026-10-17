#!/usr/bin/env bash
# The one gate for people, agents and CI. Never weaken a check to make it pass.
set -euo pipefail
cd "$(dirname "$0")/.."

# The Mac app needs Tauri's system libraries, so it is only built on macOS.
skip=""
[[ "$(uname)" == Darwin ]] || skip="--exclude freemiele-app"

cargo fmt --all --check
cargo test --workspace $skip
cargo clippy --workspace $skip --all-targets -- -D warnings

# Real serial numbers live only in git-ignored fixtures/*.json on the owner's machine.
fab() { grep -ho '"fabNumber" *: *"[0-9]*"' /dev/null "$@" 2>/dev/null | grep -o '[0-9]\{6,\}' | sort -u || true; }
serials=$(comm -23 <(fab $(ls fixtures/*.json | grep -v '\.sample\.json$')) <(fab fixtures/*.sample.json))
for s in $serials; do
  if git grep -qF "$s" || git ls-files --others --exclude-standard -z | xargs -0 grep -lF "$s" 2>/dev/null | grep -q .; then
    echo "A real serial number is in a file git would publish." >&2
    exit 1
  fi
done

echo "ALL CHECKS PASSED"
