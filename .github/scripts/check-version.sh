#!/usr/bin/env bash
# Usage: check-version.sh <tag>
# Fails unless the tag matches the Cargo.toml version and CHANGELOG.md has a
# non-empty section for it. Writes that section to notes.md.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <tag>" >&2
  exit 2
fi
tag=$1
version=${tag#v}

cargo_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
if [ "$version" != "$cargo_version" ]; then
  echo "::error::tag $tag does not match Cargo.toml version $cargo_version"
  exit 1
fi

awk -v v="$version" '
  index($0, "## [" v "]") == 1 { on = 1; next }
  on && /^## \[/ { exit }
  on { print }
' CHANGELOG.md > notes.md
if ! test -s notes.md; then
  echo "::error::CHANGELOG.md has no section for $version"
  exit 1
fi
