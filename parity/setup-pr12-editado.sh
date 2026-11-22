#!/bin/sh
set -eu
sed -i 's|"title": *"[^"]*"|"title": "editado a mao"|' "$HOME/work/acme__demo/postmortems/pr-12.json"
