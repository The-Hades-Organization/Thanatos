#!/bin/sh
set -eu
printf '#!/usr/bin/env bash\necho "queda na fonte" >&2\nexit 3\n' > "$HOME/packs/acme/env/commands/thanatos-source.sh"
