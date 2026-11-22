#!/bin/sh
set -eu
printf '#!/usr/bin/env bash\necho "isto nao e json"\n' > "$HOME/packs/acme/env/commands/thanatos-source.sh"
