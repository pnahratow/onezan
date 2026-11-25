#!/bin/sh
# Everything to run before a commit (there is no CI): formatting, clippy for Linux and Windows,
# and the tests on Linux and, as Windows binaries, under Wine.
# Usage: build/check.sh [--quick]   (--quick: Linux only, no Wine)
# Tests that need a VST3 plugin, the built-in piano (build/fetch-piano.sh) or a MIDI port skip
# themselves when the machine lacks it.
set -eu
cd "$(dirname "$0")/.."
step() { printf '\n== %s\n' "$*"; }

step "format"
cargo fmt --all --check

step "clippy (Linux)"
cargo clippy --workspace --all-targets --locked -- -D warnings

step "tests (Linux)"
cargo test --workspace --locked

if [ "${1:-}" = "--quick" ]; then
  echo; echo "ok (quick: Windows not checked)"
  exit 0
fi

target=x86_64-pc-windows-gnu
step "clippy (Windows)"
cargo clippy --workspace --all-targets --locked --target "$target" -- -D warnings

step "tests (Windows, under Wine)"
cargo test --workspace --locked --target "$target"

echo; echo "ok"
