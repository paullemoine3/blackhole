#!/usr/bin/env bash
# Builds dist/BlackHole.app: an ad-hoc signed bundle that registers as a web browser so macOS
# can offer it as the default one (System Settings > Desktop & Dock > Default web browser).
#
#   scripts/package-macos.sh            build + bundle
#   scripts/package-macos.sh --install  also copy to /Applications
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
APP=dist/BlackHole.app

cargo build --release -p blackhole
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/blackhole "$APP/Contents/MacOS/BlackHole"

ICONSET=$(mktemp -d)/BlackHole.iconset
mkdir -p "$ICONSET"
python3 scripts/make_icon.py "$ICONSET/base.png"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ICONSET/base.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$ICONSET/base.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
rm "$ICONSET/base.png"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/BlackHole.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>BlackHole</string>
  <key>CFBundleDisplayName</key><string>BlackHole</string>
  <key>CFBundleIdentifier</key><string>dev.blackhole.browser</string>
  <key>CFBundleExecutable</key><string>BlackHole</string>
  <key>CFBundleIconFile</key><string>BlackHole</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Web site URL</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>CFBundleURLSchemes</key>
      <array><string>http</string><string>https</string></array>
    </dict>
  </array>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>HTML document</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSItemContentTypes</key>
      <array><string>public.html</string><string>public.xhtml</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
plutil -lint "$APP/Contents/Info.plist"
echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
  rm -rf /Applications/BlackHole.app
  cp -R "$APP" /Applications/BlackHole.app
  LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  # The build copy would otherwise be registered as a second, competing handler.
  "$LSREGISTER" -u "$APP" || true
  "$LSREGISTER" -f /Applications/BlackHole.app
  echo "Installed to /Applications/BlackHole.app: use the Command Bar action \"Make BlackHole the default browser\"."
fi
