#!/usr/bin/env bash
# Builds the macOS disk image: opening it shows BlackHole.app next to a link to /Applications,
# over a background with an arrow, so installing is a drag and drop.
#
#   scripts/make-dmg.sh [APP] [OUT]     defaults: dist/BlackHole.app, dist/BlackHole-macos-universal.dmg
#
# The Finder layout (icon positions, background) is set through AppleScript. If Finder cannot be
# driven (no GUI session, automation refused), the image is still built with the Applications
# link, just without the arranged window: a warning says so.
set -euo pipefail

cd "$(dirname "$0")/.."
APP=${1:-dist/BlackHole.app}
OUT=${2:-dist/BlackHole-macos-universal.dmg}
VOLNAME=BlackHole
NAME=$(basename "$APP")

[[ -d "$APP" ]] || { echo "No $APP: run scripts/package-macos.sh first" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/stage"
mkdir -p "$STAGE/.background"
ditto "$APP" "$STAGE/$NAME"
ln -s /Applications "$STAGE/Applications"

python3 scripts/make_dmg_background.py "$WORK/bg.png" 1
python3 scripts/make_dmg_background.py "$WORK/bg@2x.png" 2
tiffutil -cathidpicheck "$WORK/bg.png" "$WORK/bg@2x.png" -out "$STAGE/.background/background.tiff" >/dev/null

if [[ -f "$APP/Contents/Resources/BlackHole.icns" ]]; then
  cp "$APP/Contents/Resources/BlackHole.icns" "$STAGE/.VolumeIcon.icns"
fi

# Writable image first: Finder stores the window layout in the volume's .DS_Store.
SIZE_MB=$(( $(du -sm "$STAGE" | cut -f1) + 20 ))
hdiutil create -quiet -srcfolder "$STAGE" -volname "$VOLNAME" -fs HFS+ -format UDRW \
  -size "${SIZE_MB}m" -ov "$WORK/rw.dmg"
ATTACHED=$(hdiutil attach -readwrite -noverify -noautoopen "$WORK/rw.dmg")
DEVICE=$(echo "$ATTACHED" | grep -E '^/dev/' | head -1 | awk '{print $1}')
MOUNT=$(echo "$ATTACHED" | grep -o '/Volumes/.*' | tail -1)
[[ -n "$DEVICE" && -d "$MOUNT" ]] || { echo "Could not mount the disk image" >&2; exit 1; }
trap 'hdiutil detach -quiet -force "$DEVICE" 2>/dev/null; rm -rf "$WORK"' EXIT
DISK=$(basename "$MOUNT")

# The volume shows the app icon (custom icon flag), when Xcode's SetFile is there.
if [[ -f "$MOUNT/.VolumeIcon.icns" ]] && command -v SetFile >/dev/null; then
  SetFile -c icnC "$MOUNT/.VolumeIcon.icns" || true
  SetFile -a C "$MOUNT" || true
fi

layout() {
  osascript <<APPLESCRIPT
tell application "Finder"
  tell disk "$DISK"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {200, 120, 860, 520}
    set opts to the icon view options of container window
    set arrangement of opts to not arranged
    set icon size of opts to 128
    set text size of opts to 13
    set background picture of opts to file ".background:background.tiff"
    set position of item "$NAME" of container window to {170, 190}
    set position of item "Applications" of container window to {490, 190}
    close
    open
    update without registering applications
    delay 1
    close
  end tell
end tell
APPLESCRIPT
}

# Finder may wait forever for an automation approval: give it a minute.
layout &
PID=$!
for _ in $(seq 60); do kill -0 "$PID" 2>/dev/null || break; sleep 1; done
if kill -0 "$PID" 2>/dev/null; then
  kill "$PID" 2>/dev/null || true
  echo "warning: Finder did not answer, the disk image window is not arranged" >&2
elif ! wait "$PID"; then
  echo "warning: Finder layout failed, the disk image window is not arranged" >&2
fi

chmod -Rf go-w "$MOUNT" || true
rm -rf "$MOUNT/.fseventsd"
sync
for i in 1 2 3 4 5; do
  hdiutil detach -quiet "$DEVICE" && break
  [[ $i == 5 ]] && hdiutil detach -force "$DEVICE"
  sleep 2
done

rm -f "$OUT"
trap 'rm -rf "$WORK"' EXIT
hdiutil convert -quiet "$WORK/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$OUT"
echo "Built $OUT"
