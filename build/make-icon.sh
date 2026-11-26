#!/bin/sh
# Renders the icons from assets/icon/onezan.svg (needs rsvg-convert, ImageMagick and Python 3):
# - onezan.png, 256 px: the AppImage;
# - onezan-128.png: the window icon. On X11 a 256 px one is too big for one request (256 KiB)
#   and the window ends up with no icon;
# - onezan.ico: the Windows exe. ImageMagick writes 16 to 128 px (as BMP), the 256 px image goes
#   in as PNG, which Windows reads at that size, so the file stays small.
set -eu
cd "$(dirname "$0")/../assets/icon"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for s in 16 24 32 48 64 128 256; do
  rsvg-convert -w "$s" -h "$s" onezan.svg -o "$tmp/$s.png"
done
cp "$tmp/256.png" onezan.png
cp "$tmp/128.png" onezan-128.png
magick "$tmp/16.png" "$tmp/24.png" "$tmp/32.png" "$tmp/48.png" "$tmp/64.png" "$tmp/128.png" "$tmp/small.ico"
python3 -I - "$tmp/small.ico" "$tmp/256.png" onezan.ico <<'EOF'
import struct, sys
small, png256, out = sys.argv[1:]
b = open(small, "rb").read()
count = struct.unpack_from("<HHH", b, 0)[2]
entries = []
for i in range(count):
    *fields, size, offset = struct.unpack_from("<BBBBHHII", b, 6 + 16 * i)
    entries.append((fields, b[offset:offset + size]))
entries.append(([0, 0, 0, 0, 1, 32], open(png256, "rb").read()))  # 0 means 256
offset = 6 + 16 * len(entries)
directory, data = b"", b""
for fields, blob in entries:
    directory += struct.pack("<BBBBHHII", *fields, len(blob), offset + len(data))
    data += blob
open(out, "wb").write(struct.pack("<HHH", 0, 1, len(entries)) + directory + data)
EOF
echo "made assets/icon/onezan.png, onezan-128.png and onezan.ico"
