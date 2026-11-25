#!/bin/sh
# Downloads the built-in piano into assets/piano/, where the build embeds it in the program
# (apps/onezan/build.rs). Without it the program builds, but has no built-in piano.
#
# FreePats "Upright Piano KW" (2022-02-21), a Kawai upright, SF2, stereo, 2 velocity layers:
# https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html
# Creative Commons CC0 1.0 (public domain); its cc0.txt and readme.txt are kept next to it.
#
# Needs curl, sha256sum, and bsdtar (libarchive) or 7z to unpack the .7z archive.
# Does nothing when the piano is already there.
set -eu
cd "$(dirname "$0")/.."
name=UprightPianoKW-SF2-20220221
url="https://freepats.zenvoid.org/Piano/UprightPianoKW/$name.7z"
sha256=17c084c6e4205233dc49b34e4bc44a9b2d7c7a2c02b04729ecda77079b07c826
dest=assets/piano
sf2="$dest/UprightPianoKW-20220221.sf2"

if [ -f "$sf2" ]; then
  echo "built-in piano: $sf2 (already there)"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "downloading $url"
curl -fL --retry 3 -o "$tmp/piano.7z" "$url"
echo "$sha256  $tmp/piano.7z" | sha256sum -c --quiet - || {
  echo "the download does not match the expected checksum; not using it" >&2
  exit 1
}
mkdir -p "$tmp/x"
if command -v bsdtar >/dev/null; then
  bsdtar -xf "$tmp/piano.7z" -C "$tmp/x"
elif command -v 7z >/dev/null; then
  7z x -bd -o"$tmp/x" "$tmp/piano.7z" >/dev/null
elif command -v 7za >/dev/null; then
  7za x -bd -o"$tmp/x" "$tmp/piano.7z" >/dev/null
else
  echo "need bsdtar (libarchive) or 7z to unpack the archive" >&2
  exit 1
fi
mkdir -p "$dest"
cp "$tmp/x/$name/UprightPianoKW-20220221.sf2" "$tmp/x/$name/cc0.txt" "$tmp/x/$name/readme.txt" "$dest/"
echo "built-in piano: $sf2"
