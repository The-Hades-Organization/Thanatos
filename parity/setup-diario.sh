#!/bin/sh
set -eu
dir="$HOME/work/acme__demo/journal"
mkdir -p "$dir"
{
  echo '{"v":1,"ts":"2026-03-01T13:00:00Z","source":"orch","agent":"impl-x","kind":"decision","summary":"retry com backoff"}'
  echo '{"v":1,"ts":"2026-03-01T13:30:00Z","source":"orch","agent":"impl-x","kind":"tool_pre","summary":"ruido"}'
  echo 'linha quebrada {'
  echo '{"v":1,"ts":"2026-03-01T14:30:00Z","source":"orch","agent":"impl-x","kind":"note","summary":"gateway ok"}'
  echo '{"v":1,"ts":"2026-01-01T14:30:00Z","source":"orch","agent":"impl-x","kind":"note","summary":"fora da janela"}'
} > "$dir/impl-x.jsonl"
