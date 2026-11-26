# Building onezan

## Requirements

Rust via [rustup](https://rustup.rs): the version pinned in `rust-toolchain.toml` (1.100.0), which rustup picks up by itself. On Linux, building needs the ALSA
and JACK development files and `pkg-config`. JACK's are needed only to compile: the app loads
JACK at runtime and runs without it.

| System | Install |
|---|---|
| Debian, Ubuntu, Linux Mint | `sudo apt install libasound2-dev libjack-jackd2-dev pkg-config` |
| Arch | `sudo pacman -S alsa-lib pipewire-jack pkgconf` (or `jack2` instead of `pipewire-jack`) |

Windows needs nothing beyond Rust.

## The built-in piano

The built-in piano is a 57 MB SoundFont, embedded in the program but not kept in git. Fetch it
once (needs `curl` and `bsdtar` or `7z`); without it the program builds, with a warning, and has no
built-in piano. The release scripts below fetch it themselves.

```sh
build/fetch-piano.sh        # -> assets/piano/
```

## Build and run

```sh
cargo run --release                    # the window
cargo test --workspace                 # the tests
cargo clippy --all-targets             # strict lints (see Cargo.toml)
```

The command line, for checks without the window:

```sh
cargo run --release -- list            # installed VST3 instruments (--all for effects too)
cargo run --release -- midi            # MIDI input ports
cargo run --release -- play            # without the window
cargo run --release -- play "Surge XT" --midi "Keystation"
cargo run --release -- play --demo     # plays a C major scale through the plugin
cargo run --release -- play --metronome 90
cargo run --release -- play built-in   # the built-in piano, even when plugins are installed
cargo run --release -- render          # offline render check, no audio device needed
```

## Linux release build

A binary runs only on systems whose glibc is at least as new as the one it was built on. For a
binary that runs on most distributions (glibc 2.30+: Ubuntu 20.04+, Mint 20+, Debian 11+,
Fedora 32+, Arch, openSUSE), build it in a container with rootless Podman (or Docker):

```sh
build/linux-release.sh      # -> target/linux-release/release/onezan
```

It needs only Podman on the build machine, and only glibc and ALSA (`libasound.so.2`) on the
machine it runs on; graphics and JACK are loaded at runtime. `ENGINE=docker` uses Docker even
when Podman is installed.

Then package it as a tarball (binary, README, licences) and an AppImage (with the icon and menu
entry from `assets/icon/` and `build/linux/onezan.desktop`):

```sh
build/linux-package.sh [DIR]   # -> target/linux-package/onezan-VERSION-COMMIT-{linux-x64.tar.gz,x86_64.AppImage}
```

It downloads appimagetool and the AppImage runtime (pinned versions, checked by SHA-256) into
`target/appimage-tools/` the first time.

## Windows release build

The Windows build is cross-compiled on Linux with MinGW (`wine` and the MinGW cross-compiler,
e.g. Arch's `mingw-w64-gcc`). `cargo test --target x86_64-pc-windows-gnu --workspace` builds the
tests for Windows and runs them under Wine.

The release zip (the exe, needing only DLLs that come with Windows 10 and 11, with the README
and the licences) is built the same way:

```sh
build/windows-release.sh [DIR]   # -> target/windows-release/onezan-VERSION-COMMIT-windows-x64.zip
```

With a folder given, the zip is copied there too. A build from uncommitted changes is named
`COMMIT-dirty`.

## GitHub Actions

- **CI** (`.github/workflows/ci.yml`): `build/check.sh --quick` (formatting, clippy, tests on
  Linux) on every push to master and every pull request.
- **Release** (`.github/workflows/release.yml`): the Linux and Windows builds above. Pushing a
  tag `v*` builds everything and makes a **draft** release with the files, to check and publish
  by hand. Run by hand (Actions → Release → Run workflow) it only builds; the files are the run's
  artifacts.

## Icon

`assets/icon/onezan.svg` (dragon scales: scales, as in piano scales) is the source.
`build/make-icon.sh` renders the rest from it (rsvg-convert, ImageMagick, Python 3); run it again
after changing the SVG and commit what it makes:

- `onezan.png` (256 px): the AppImage.
- `onezan-128.png`: the window icon, embedded in the binary. On Linux the window is always an
  X11 one (XWayland under Wayland), and a 256 px icon is too big for one X11 request: the
  window would have none.
- `onezan.ico` (16 to 256 px): the Windows exe's icon. `apps/onezan/build.rs` compiles it into
  the exe with MinGW's `windres` (`x86_64-w64-mingw32-windres`, or `$WINDRES`).

## Layout

| Crate | Role |
|---|---|
| `crates/vst3-host` | A small VST3 host on the `vst3` bindings crate: scan, load, wire component and controller, process notes and MIDI CCs (through `IMidiMapping`). |
| `crates/engine` | MIDI input (`midir`), audio output (`cpal`), lock-free queues (`rtrb`) between them, and offline rendering. |
| `apps/onezan` | The app: the window (egui) and the command line. |

See `docs/STATUS.md` for decisions, progress and next steps, and `docs/TESTING.md` for how the
tests are organised.
