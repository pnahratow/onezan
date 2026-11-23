# onezan

Play your MIDI keyboard through a real VST3 instrument, on Linux and Windows.
The long-term goal is a scale trainer with fingering display, a learning order, a metronome and
progress tracking. This first milestone is the instrument host it will sit on.

## Use

```sh
cargo run --release -- list            # installed VST3 instruments (--all for effects too)
cargo run --release -- midi            # MIDI input ports
cargo run --release -- play            # first instrument, first keyboard
cargo run --release -- play "Surge XT" --midi "Keystation"
cargo run --release -- play --demo     # plays a C major scale through the plugin
cargo run --release -- render          # offline render check, no audio device needed
```

Plugins are searched in `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3` (Linux) and
`%CommonProgramFiles%\VST3` (Windows), plus any folders in `$VST3_PATH`.

## Layout

| Crate | Role |
|---|---|
| `crates/vst3-host` | A small VST3 host on the `vst3` bindings crate: scan, load, wire component and controller, process notes and MIDI CCs (through `IMidiMapping`). |
| `crates/engine` | MIDI input (`midir`), audio output (`cpal`), lock-free queues (`rtrb`) between them, and offline rendering. |
| `apps/onezan` | The command-line app. |

## Not done yet

- Plugin editor windows (X11 on Linux, HWND on Windows), plugin state saving, presets.
- CLAP hosting (`clack-host`), a built-in fallback sound.
- macOS module loading.

## Status

See `docs/STATUS.md` for decisions, progress and next steps.

## License

GPL-3.0-or-later. See `LICENSE`.
