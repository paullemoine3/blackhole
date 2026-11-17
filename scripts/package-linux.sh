#!/usr/bin/env bash
# Packages the Linux build (x86_64): dist/BlackHole-linux-x86_64.tar.gz (binary, desktop entry,
# icon) and dist/BlackHole-linux-amd64.deb (installs to /usr, depends on WebKitGTK 4.1).
# Usage: scripts/package-linux.sh VERSION [BINARY]   (BINARY: target/release/blackhole)
# The app never updates itself on Linux: the package manager (or the user) replaces it.
set -euo pipefail
cd "$(dirname "$0")/.."
version=${1:?version}
bin=${2:-target/release/blackhole}
out=dist
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$out"

python3 scripts/make_icon.py "$stage/blackhole.png"

desktop() {
  cat <<EOF
[Desktop Entry]
Type=Application
Name=BlackHole
GenericName=Web Browser
Comment=Privacy-first browser
Exec=$1 %U
Icon=blackhole
Terminal=false
Categories=Network;WebBrowser;
MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;
StartupWMClass=blackhole
EOF
}

# Portable archive.
tar_root="$stage/BlackHole"
mkdir -p "$tar_root"
install -m 755 "$bin" "$tar_root/blackhole"
install -m 644 "$stage/blackhole.png" "$tar_root/blackhole.png"
desktop blackhole > "$tar_root/blackhole.desktop"
tar -C "$stage" -czf "$out/BlackHole-linux-x86_64.tar.gz" BlackHole

# Debian package.
deb="$stage/deb"
install -Dm 755 "$bin" "$deb/usr/bin/blackhole"
install -Dm 644 "$stage/blackhole.png" "$deb/usr/share/icons/hicolor/1024x1024/apps/blackhole.png"
mkdir -p "$deb/usr/share/applications" "$deb/DEBIAN"
desktop /usr/bin/blackhole > "$deb/usr/share/applications/blackhole.desktop"
size=$(du -sk "$deb/usr" | cut -f1)
cat > "$deb/DEBIAN/control" <<EOF
Package: blackhole
Version: $version
Section: web
Priority: optional
Architecture: amd64
Installed-Size: $size
Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libxkbcommon-x11-0, libdbus-1-3
Recommends: gnome-keyring | kwalletmanager
Maintainer: BlackHole <noreply@github.com>
Homepage: https://github.com/paullemoine3/blackhole
Description: Privacy-first browser powered by WebKitGTK
 BlackHole is an Arc-inspired browser with spaces, folders, the Easel and
 built-in tracker blocking, on the system's web engine.
EOF
dpkg-deb --root-owner-group --build "$deb" "$out/BlackHole-linux-amd64.deb" >/dev/null
ls -l "$out"/BlackHole-linux-*
