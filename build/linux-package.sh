#!/bin/sh
# Packages the Linux release binary (from build/linux-release.sh) two ways:
#   target/linux-package/onezan-VERSION-COMMIT-linux-x64.tar.gz   the binary, README, licences
#   target/linux-package/onezan-VERSION-COMMIT-x86_64.AppImage    one file, with icon and menu entry
# Downloads appimagetool and the AppImage runtime (pinned versions, checked by SHA-256) into
# target/appimage-tools/ the first time. Needs curl and sha256sum; no FUSE (appimagetool runs
# extracted).
# Usage: build/linux-package.sh [DIR]   (also copies both files into DIR)
set -eu
cd "$(dirname "$0")/.."
bin=target/linux-release/release/onezan
[ -x "$bin" ] || { echo "no $bin: run build/linux-release.sh first" >&2; exit 1; }

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
commit=$(git rev-parse --short HEAD)
git diff --quiet HEAD || commit="$commit-dirty"
out=target/linux-package
rm -rf "$out"
mkdir -p "$out"

# The tarball.
name="onezan-$version-$commit-linux-x64"
mkdir -p "$out/$name"
strip -o "$out/$name/onezan" "$bin"
# The GPL asks for the licence to travel with the program; the OFL the same for Bravura.
cp README.md LICENSE "$out/$name/"
cp assets/fonts/OFL.txt "$out/$name/Bravura-OFL.txt"
tar -C "$out" -czf "$out/$name.tar.gz" "$name"

# The AppImage tools, pinned.
tools=target/appimage-tools
mkdir -p "$tools"
fetch() { # url file sha256
  if ! echo "$3  $tools/$2" | sha256sum -c --quiet - 2>/dev/null; then
    curl -fL --retry 3 -o "$tools/$2" "$1"
    echo "$3  $tools/$2" | sha256sum -c --quiet - || {
      echo "$2 does not match the expected checksum; not using it" >&2
      rm -f "$tools/$2"
      exit 1
    }
  fi
}
fetch https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
  appimagetool ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
fetch https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 \
  runtime-x86_64 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
chmod +x "$tools/appimagetool"

# The AppDir: the program, its menu entry and icon (at the top, where AppImages look), the licences.
app="$out/AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps" \
  "$app/usr/share/doc/onezan"
cp "$out/$name/onezan" "$app/usr/bin/"
cp build/linux/onezan.desktop "$app/usr/share/applications/"
cp build/linux/onezan.desktop "$app/"
cp assets/icon/onezan.png "$app/usr/share/icons/hicolor/256x256/apps/"
cp assets/icon/onezan.png "$app/"
cp README.md LICENSE "$app/usr/share/doc/onezan/"
cp assets/fonts/OFL.txt "$app/usr/share/doc/onezan/Bravura-OFL.txt"
ln -s usr/bin/onezan "$app/AppRun"

image="$out/onezan-$version-$commit-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool" --no-appstream \
  --runtime-file "$tools/runtime-x86_64" "$app" "$image"
rm -rf "$app" "$out/$name"

echo "built: $out/$name.tar.gz ($(du -h "$out/$name.tar.gz" | cut -f1))"
echo "built: $image ($(du -h "$image" | cut -f1))"
if [ $# -gt 0 ]; then
  cp "$out/$name.tar.gz" "$image" "$1/"
  echo "copied to $1/"
fi
