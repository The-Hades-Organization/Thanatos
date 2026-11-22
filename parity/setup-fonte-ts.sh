#!/bin/sh
set -eu
env="$HOME/packs/acme/env"
rm "$env/commands/thanatos-source.sh"
cat > "$env/commands/thanatos-source.ts" <<'TS'
console.log(JSON.stringify({ v: 1, events: [{ ts: "2026-03-01T14:10:00Z", text: "retry ativo" }], metrics: [{ name: "p99", value: 3.2 }] }));
TS
sed -i 's|thanatos-source.sh|thanatos-source.ts|' "$env/plugin.json"
