#!/bin/sh
# Prints the Nix SRI hash of each URL (as fetchurl expects). Run in nixos/nix via build.ps1.
set -eu
for url in "$@"; do
  hash=$(nix-prefetch-url --type sha256 "$url" 2>/dev/null)
  echo "$url"
  echo "  $(nix --extra-experimental-features nix-command hash convert --hash-algo sha256 --to sri "$hash")"
done
