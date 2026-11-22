#!/bin/sh
# Repo de `cwd = "repo"` com main e uma branch `feat`, em datas e identidade fixas.
set -eu
dir="$HOME/repo"
export GIT_AUTHOR_NAME=ana GIT_AUTHOR_EMAIL=ana@example.invalid GIT_COMMITTER_NAME=ana GIT_COMMITTER_EMAIL=ana@example.invalid
g() { git -C "$dir" "$@"; }
commit() {
  GIT_AUTHOR_DATE="$1" GIT_COMMITTER_DATE="$1" g commit -q -m "$2"
}
g checkout -q -b main
mkdir -p "$dir/src"
echo base > "$dir/README.md"
g add -A
commit 2026-02-20T10:00:00Z "chore: base"
g checkout -q -b feat
echo a > "$dir/src/gateway.ts"
g add -A
commit 2026-03-01T09:00:00Z "feat: gateway"
echo b >> "$dir/src/gateway.ts"
echo c > "$dir/src/pay.ts"
g add -A
commit 2026-03-01T15:00:00Z "fix: retry no gateway"
