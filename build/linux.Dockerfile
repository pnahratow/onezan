# The Linux release build: compiled against Ubuntu 20.04's glibc (2.31), for the widest reach
# without going ancient. The binary runs on Ubuntu 20.04+, Linux Mint 20+, Debian 11+,
# Fedora 32+, current Arch and openSUSE. Used by build/linux-release.sh.
#
# Ubuntu 20.04 rather than Debian 11 (same glibc): its package archive stays online while it is
# in extended support (to 2030), so the build keeps working.
FROM docker.io/library/ubuntu:20.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl build-essential pkg-config libasound2-dev libjack-jackd2-dev \
 && rm -rf /var/lib/apt/lists/*

# Rust in /opt, readable by the (non-root) user the build runs as.
ENV RUSTUP_HOME=/opt/rustup PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | CARGO_HOME=/opt/cargo sh -s -- -y --profile minimal --default-toolchain stable \
 && chmod -R a+rX /opt/rustup /opt/cargo
