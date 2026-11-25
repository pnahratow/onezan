#!/bin/sh
# The Linux release build, made in a container against an older glibc (see build/linux.Dockerfile)
# so that it runs on as many distributions as possible. Uses rootless Podman if installed (no
# root, no daemon), else Docker.
# Output: target/linux-release/release/onezan
set -eu
cd "$(dirname "$0")/.."
# The built-in piano, embedded in the binary (fetched here: the container sees the repository).
build/fetch-piano.sh
image=onezan-build-linux
if command -v podman >/dev/null; then
  engine=podman
  # Rootless: the container's user is mapped to the calling user, so files in target/ stay theirs.
  as_user="--userns=keep-id"
else
  engine=docker
  as_user="-u $(id -u):$(id -g)"
fi
"$engine" build -t "$image" -f build/linux.Dockerfile build
# Crates are cached in target/, so later builds do not download them again.
# shellcheck disable=SC2086 # $as_user is one or two words on purpose
"$engine" run --rm $as_user -v "$PWD":/src -w /src \
  -e CARGO_HOME=/src/target/container-cargo-home \
  -e CARGO_TARGET_DIR=/src/target/linux-release \
  "$image" cargo build --release --locked
bin=target/linux-release/release/onezan
echo "built: $bin"
echo "needs glibc $(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1 | cut -d_ -f2) or newer"
