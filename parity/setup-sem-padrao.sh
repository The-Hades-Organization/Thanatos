#!/bin/sh
set -eu
sed -i '/"card"/d' "$HOME/packs/acme/profile.json"
