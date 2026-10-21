#!/usr/bin/env bash
# Builds zappp and wraps it in a signed .app so macOS permissions
# (Microphone, Accessibility, Input Monitoring) attach to a stable identity.
#   scripts/bundle.sh            # debug build → target/zappp.app
#   scripts/bundle.sh --release  # release build
#   scripts/bundle.sh --install  # also replace /Applications/zappp.app
set -euo pipefail
cd "$(dirname "$0")/.."

install=false
args=()
for arg in "$@"; do
  if [[ "$arg" == "--install" ]]; then install=true; else args+=("$arg"); fi
done
set -- "${args[@]+"${args[@]}"}"

profile=debug
if [[ "${1:-}" == "--release" ]]; then
  cargo build --release
  profile=release
else
  cargo build
fi

app=target/zappp.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/zappp" "$app/Contents/MacOS/zappp"
cp brand/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>zappp</string>
  <key>CFBundleDisplayName</key><string>zappp</string>
  <key>CFBundleIdentifier</key><string>dev.zappp.app</string>
  <key>CFBundleExecutable</key><string>zappp</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>zappp listens while you hold your dictation key and turns your speech into text.</string>
</dict>
</plist>
PLIST

# Prefer the stable local identity from scripts/make-dev-cert.sh so macOS
# keeps zappp's permissions across rebuilds; fall back to ad-hoc.
identity="${ZAPPP_SIGN_IDENTITY:-}"
if [[ -z "$identity" ]] && security find-identity -v -p codesigning 2>/dev/null | grep -q "zappp Development"; then
  identity="zappp Development"
fi
codesign --force --deep --sign "${identity:--}" "$app" >/dev/null
if [[ -z "$identity" ]]; then
  echo "note: ad-hoc signed; macOS will ask for permissions again after each rebuild (run scripts/make-dev-cert.sh once to fix)" >&2
fi
if $install; then
  pkill -f "/Applications/zappp.app/Contents/MacOS/zappp" || true
  rm -rf /Applications/zappp.app
  ditto "$app" /Applications/zappp.app
  rm -rf "$app"
  app=/Applications/zappp.app
fi
echo "$app"
