#!/bin/sh
set -eu
sed -i 's|"identity": { "repo": "acme/demo" }|"identity": { "repo": "acme/demo", "gh": { "user": "ana" } }|' "$HOME/packs/acme/profile.json"
