#!/bin/sh
set -eu
env="$HOME/packs/acme/env"
cp "$env/commands/thanatos-source.sh" "$HOME/packs/acme/fora.sh"
sed -i 's|commands/thanatos-source.sh|../fora.sh|' "$env/plugin.json"
