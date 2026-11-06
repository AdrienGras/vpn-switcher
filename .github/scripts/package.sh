#!/usr/bin/env bash
# Usage: package.sh <target> <bin> <gen-bin>
# Builds vpns-<target>.tar.gz (+ .sha256) from <bin>; <gen-bin> is a runnable
# vpns used to generate completions and the man page (it can differ from <bin>
# when cross-compiling for a target that cannot run on the host).
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <target> <bin> <gen-bin>" >&2
  exit 2
fi
target=$1
bin=$2
gen_bin=$3
name="vpns-$target"

rm -rf "$name"
mkdir -p "$name/completions"
cp "$bin" "$name/vpns"
cp README.md LICENSE CHANGELOG.md "$name/"
"$gen_bin" completions bash > "$name/completions/vpns.bash"
"$gen_bin" completions zsh > "$name/completions/_vpns"
"$gen_bin" completions fish > "$name/completions/vpns.fish"
"$gen_bin" man > "$name/vpns.1"

COPYFILE_DISABLE=1 tar czf "$name.tar.gz" "$name"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$name.tar.gz" > "$name.tar.gz.sha256"
else
  shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256"
fi
rm -rf "$name"
echo "$name.tar.gz"
