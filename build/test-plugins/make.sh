#!/bin/sh
# Builds the two crashing VST3 bundles for the hand check of "Plugins that crash onezan"
# (docs/QA.md): CrashScan.vst3 (crashes while scanned) and CrashLoad.vst3 (crashes while loading).
#
#   build/test-plugins/make.sh [linux|windows] [out dir]   (default: linux, target/test-plugins)
#
# Linux needs a C compiler; windows needs MinGW (x86_64-w64-mingw32-gcc). Then point onezan at
# the folder, with a data folder of its own so your real settings stay untouched:
#
#   VST3_PATH=target/test-plugins XDG_DATA_HOME=/tmp/onezan-qa onezan
#   (Windows: copy the folder over and set VST3_PATH to it; the data folder is %APPDATA%\onezan)
set -eu
here=$(dirname "$0")
target=${1:-linux}
out=${2:-target/test-plugins}
case $target in
linux) cc="cc -shared -fPIC -O1" arch=x86_64-linux ext=so ;;
windows) cc="x86_64-w64-mingw32-gcc -shared -O1" arch=x86_64-win ext=vst3 ;;
*) echo "usage: $0 [linux|windows] [out dir]" >&2; exit 2 ;;
esac
for name in CrashScan CrashLoad; do
  src=$here/$(echo "$name" | sed 's/Crash\(.*\)/crash-\1/' | tr 'A-Z' 'a-z').c
  dir=$out/$name.vst3/Contents/$arch
  mkdir -p "$dir"
  $cc -o "$dir/$name.$ext" "$src"
done
echo "built $out/CrashScan.vst3 and $out/CrashLoad.vst3 ($target)"
