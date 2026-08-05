#!/usr/bin/env bash
# Fetch rarlab's unrar source and vendor it into vendor/unrarsrc.
# The unrar source is NOT redistributed in this repository (see README "License");
# run this once after cloning, and again to upgrade.
set -euo pipefail
cd "$(dirname "$0")/.."

URL="${1:-}"
if [ -z "$URL" ]; then
    echo "Locating latest unrarsrc tarball on rarlab.com..."
    URL=$(curl -sL https://www.rarlab.com/rar_add.htm \
        | grep -oE 'https://www\.rarlab\.com/rar/unrarsrc-[0-9.]+\.tar\.gz' \
        | head -1)
fi
if [ -z "$URL" ]; then
    echo "error: could not find an unrarsrc URL automatically." >&2
    echo "Download unrarsrc-*.tar.gz manually from https://www.rarlab.com/rar_add.htm" >&2
    echo "and pass its URL: $0 <url>" >&2
    exit 1
fi

echo "Downloading $URL"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sL -o "$tmp/unrarsrc.tar.gz" "$URL"
tar xzf "$tmp/unrarsrc.tar.gz" -C "$tmp"
mkdir -p vendor
rm -rf vendor/unrarsrc
mv "$tmp/unrar" vendor/unrarsrc

if [ ! -f vendor/unrarsrc/dll.hpp ]; then
    echo "error: vendor/unrarsrc/dll.hpp not found after extraction" >&2
    exit 1
fi
grep -E 'RARVER_MAJOR|RARVER_MINOR' vendor/unrarsrc/version.hpp || true
echo "Vendored unrar source into vendor/unrarsrc — you can now: cargo build --release"
