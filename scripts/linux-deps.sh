#!/usr/bin/env bash
# System libraries GPUI needs to build on Debian/Ubuntu.
set -euo pipefail
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  pkg-config libssl-dev \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libvulkan-dev \
  libx11-dev libx11-xcb-dev libxcb1-dev \
  libfontconfig1-dev libfreetype-dev \
  libasound2-dev libzstd-dev \
  dpkg-dev rpm file
