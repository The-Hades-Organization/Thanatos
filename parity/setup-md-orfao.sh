#!/bin/sh
set -eu
dir="$HOME/work/acme__demo/postmortems"
mkdir -p "$dir"
echo "# sobra de uma execução interrompida" > "$dir/pr-12.md"
