#!/usr/bin/env bash
# Builds release artifacts for the version in Cargo.toml:
#   dist/zappp-<version>.zip   (what the Homebrew cask installs)
#   dist/zappp-<version>.dmg   (drag-to-Applications installer)
#   dist/SHA256SUMS
#   dist/zappp.rb              (Homebrew cask, pointing at the GitHub release)
#
# Releases are signed ad-hoc: there is no Apple Developer ID yet, and a local
# self-signed certificate wouldn't be trusted on other Macs. Run on Apple
# Silicon; CI runs this on tag pushes (.github/workflows/release.yml).
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
repo="${ZAPPP_REPO:-hiteshbandhu/zappp}"
dist=dist
rm -rf "$dist" && mkdir -p "$dist"

# ZAPPP_RELEASE_PROFILE=debug packages a debug build (to test this script).
if [[ "${ZAPPP_RELEASE_PROFILE:-release}" == release ]]; then
  ZAPPP_SIGN_IDENTITY=- scripts/bundle.sh --release
else
  ZAPPP_SIGN_IDENTITY=- scripts/bundle.sh
fi
app=target/zappp.app
codesign --verify --strict "$app"

# Zip for Homebrew (ditto keeps the bundle's metadata intact).
zip="$dist/zappp-$version.zip"
ditto -c -k --keepParent "$app" "$zip"

# DMG with an Applications shortcut for drag-and-drop installs.
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
ditto "$app" "$stage/zappp.app"
ln -s /Applications "$stage/Applications"
dmg="$dist/zappp-$version.dmg"
hdiutil create -quiet -volname "zappp $version" -srcfolder "$stage" -format UDZO -ov "$dmg"

(cd "$dist" && shasum -a 256 "zappp-$version.zip" "zappp-$version.dmg" > SHA256SUMS)
zip_sha=$(shasum -a 256 "$zip" | cut -d' ' -f1)

sed -e "s/@VERSION@/$version/g" -e "s/@SHA256@/$zip_sha/g" -e "s#@REPO@#$repo#g" \
  packaging/zappp.rb.in > "$dist/zappp.rb"

echo "Built zappp $version:"
ls -lh "$dist"
