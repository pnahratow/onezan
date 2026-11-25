#!/bin/sh
# The Windows release zip, cross-built on Linux with MinGW (Arch: `mingw-w64-gcc`; the Rust target
# x86_64-pc-windows-gnu). The exe needs only DLLs that come with Windows 10 and 11; the build
# fails if it would need a MinGW runtime DLL, which the zip does not carry.
# Usage: build/windows-release.sh [DIR]   (also copies the zip into DIR)
# Output: target/windows-release/onezan-VERSION-COMMIT-windows-x64.zip
set -eu
cd "$(dirname "$0")/.."
# The built-in piano, embedded in the exe.
build/fetch-piano.sh
target=x86_64-pc-windows-gnu
cargo build --release --locked --target "$target"
exe="target/$target/release/onezan.exe"

# MinGW's own runtime DLLs are not on a Windows PC.
if x86_64-w64-mingw32-objdump -p "$exe" | grep -i "DLL Name" | grep -Ei "libgcc|libstdc|libwinpthread"; then
  echo "error: the exe needs a MinGW runtime DLL (above); link it statically" >&2
  exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
commit=$(git rev-parse --short HEAD)
# Uncommitted changes: the commit alone does not say what is in it.
git diff --quiet HEAD || commit="$commit-dirty"
name="onezan-$version-$commit-windows-x64"
out=target/windows-release
rm -rf "${out:?}/$name" "$out/$name.zip"
mkdir -p "$out/$name"
# Without the debug symbols.
x86_64-w64-mingw32-strip -o "$out/$name/onezan.exe" "$exe"
# The GPL asks for the licence to travel with the program.
cp README.md LICENSE "$out/$name/"
(cd "$out" && zip -qr "$name.zip" "$name")
zip="$out/$name.zip"
echo "built: $zip ($(du -h "$zip" | cut -f1))"
if [ $# -gt 0 ]; then
  cp "$zip" "$1/"
  echo "copied to $1/"
fi
