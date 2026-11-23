# onezan

Play your MIDI keyboard through a real VST3 instrument, on Linux and Windows.
The long-term goal is a scale trainer with fingering display, a learning order, a metronome and
progress tracking. This first milestone is the instrument host it will sit on.

## Build

Rust (stable, edition 2024) via [rustup](https://rustup.rs). On Linux, building needs the ALSA
and JACK development files and `pkg-config`. JACK's are needed only to compile: the app loads
JACK at runtime and runs without it.

| System | Install |
|---|---|
| Debian, Ubuntu, Linux Mint | `sudo apt install libasound2-dev libjack-jackd2-dev pkg-config` |
| Arch | `sudo pacman -S alsa-lib pipewire-jack pkgconf` (or `jack2` instead of `pipewire-jack`) |

Tested on Linux Mint 22 (the Debian line) and Arch (the Arch line). Windows should need nothing
beyond Rust (not tried yet).

A binary runs only on systems whose glibc is at least as new as the one it was built on. For a
binary that runs on most distributions (glibc 2.30+: Ubuntu 20.04+, Mint 20+, Debian 11+,
Fedora 32+, Arch, openSUSE), build it in a container with rootless Podman (or Docker):

```sh
build/linux-release.sh      # -> target/linux-release/release/onezan
```

It needs only Podman on the build machine, and only glibc and ALSA (`libasound.so.2`) on the
machine it runs on; graphics and JACK are loaded at runtime.

On Linux, the Windows build can be checked with Wine (`wine` and the MinGW cross-compiler, e.g.
Arch's `mingw-w64-gcc`): `cargo test --target x86_64-pc-windows-gnu --workspace` builds the tests
for Windows and runs them under Wine.

The Windows release zip (the exe, needing only DLLs that come with Windows 10 and 11, with this
README and the licence) is cross-built the same way:

```sh
build/windows-release.sh [DIR]   # -> target/windows-release/onezan-VERSION-COMMIT-windows-x64.zip
```

With a folder given, the zip is copied there too. A build from uncommitted changes is named
`COMMIT-dirty`.

## Use

```sh
cargo run --release                    # the window: first instrument, first keyboard
cargo run --release -- list            # installed VST3 instruments (--all for effects too)
cargo run --release -- midi            # MIDI input ports
cargo run --release -- play            # without the window
cargo run --release -- play "Surge XT" --midi "Keystation"
cargo run --release -- play --demo     # plays a C major scale through the plugin
cargo run --release -- play --metronome 90   # with the metronome (the window has it too)
cargo run --release -- render          # offline render check, no audio device needed
```

Plugins are searched in `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3` (Linux) and
`%CommonProgramFiles%\VST3` (Windows), plus any folders in `$VST3_PATH`.

On Debian, Ubuntu and Mint, PipeWire's JACK library is not on the library path: for the lowest
latency, install `pipewire-jack` and start with `pw-jack onezan` (the app says so when it
applies).

## Where the fingerings come from

onezan shows the standard fingering taught for each scale, for both hands, in all 12 keys:
major, natural minor, harmonic minor and melodic minor. It doesn't invent fingerings. Every scale
was compared with at least two independent published charts:
[pianoscales.org](https://pianoscales.org/major.html),
[Hoffman Academy's Melodic Minor Scale Fingering Guide](https://cdn.hoffmanacademy.com/blog/wp-content/uploads/2022/04/Melodic-Minor-Scale-Fingering-Guide.pdf),
[Greg Spero's scale lessons](https://composition.gregspero.com/lessons/a3/),
[musicandhealth.co.uk](https://musicandhealth.co.uk/scales_examples/fingering1.htm), and a
[teaching chart from the University of Miami](https://www.cs.miami.edu/home/burt/music/finger-charts.html)
that shows where the fourth finger falls in every key. The sources agree on almost every note.
Where they differ, it's only about which finger starts or ends a scale in a few keys. In those
cases onezan follows the majority, and the code records each case and the reason. The numbers
from these charts are built into onezan's automated tests, so the tests fail if the fingerings
ever change by accident. The tests also check that every scale can be played up and down over
one to four octaves: the thumb only lands on white keys, and the hand only moves by passing the
thumb under or crossing over it. Many good fingerings exist, and teachers and exam boards
sometimes prefer others. If your teacher gives you a different one, follow your teacher.

## Layout

| Crate | Role |
|---|---|
| `crates/vst3-host` | A small VST3 host on the `vst3` bindings crate: scan, load, wire component and controller, process notes and MIDI CCs (through `IMidiMapping`). |
| `crates/engine` | MIDI input (`midir`), audio output (`cpal`), lock-free queues (`rtrb`) between them, and offline rendering. |
| `apps/onezan` | The app: the window (egui) and the command line. |

## Not done yet

- The plugin's editor on Windows (HWND), presets.
- CLAP hosting (`clack-host`), a built-in fallback sound.
- macOS module loading.

## Status

See `docs/STATUS.md` for decisions, progress and next steps.

## License

GPL-3.0-or-later. See `LICENSE`.
