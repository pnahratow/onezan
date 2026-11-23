# onezan: status and handoff

Last updated: 2026-11-23. Read this first when resuming.

## What onezan is

A desktop app for Linux and Windows. It reads a MIDI keyboard and plays it through a VST3
instrument. The long-term goal is a scale trainer: an on-screen keyboard with fingering numbers,
a scale picker, a suggested learning order, a metronome and progress tracking.

## Decisions made

| Topic | Decision |
|---|---|
| Language | Rust. JUCE/C++ was considered and rejected. |
| License | Open source and copyleft-compatible. The app is **GPL-3.0-or-later**: Apache-2.0 dependencies are compatible with GPLv3, not with GPLv2-only. |
| Plugin hosting | **Option C:** our own thin host on the `vst3` bindings crate (coupler-rs, v0.3), behind our own API. CLAP via `clack-host` later. |
| Rejected as host libraries | `vst3-host` (one maintainer, little Linux testing with real plugins), `maolan-plugin-host` (too new), `rack` (macOS-first, no commits since Nov 2025), `plughost` (no Linux). |
| First milestone | A standalone "play your keyboard through a real VST" app. The user sees this as a worthwhile app on its own; the scale trainer builds on it. |
| Out of v1 | Grading or judging performance (timing and accuracy scores). |
| Fingering | MIDI cannot tell which finger was used. The app shows fingering but cannot check it. Fingerings will come from a hand-written data table, not an algorithm. |

## Reference code (consulted, not dependencies)

All of these use `vst3` 0.3 and are licensed compatibly with GPL-3:

- **transit**, https://github.com/transit-audio/transit (Apache-2.0). `crates/vst3-host/src/ffi/` holds
  `module.rs` (loading, entry points), `instance.rs` (set-up, `process`), `events.rs`,
  `host_context.rs`, `host_objects.rs` (IMessage, IAttributeList, MemoryStream/IBStream),
  `view.rs` and `run_loop.rs` (**the editor window and Linux IRunLoop: look here for the next step**).
- **suhina**, https://github.com/suhina-daw/suhina-daw (GPL-3.0). `crates/plugin-host` uses
  clack-host and vst3, and runs plugins out of process.
- Maolan (BSD-2), Lemon Studio (GPL-3), tripplyons/daw (MIT).
- Gridline (C++, GPL-3) has its own hosting code on Steinberg's raw headers. Not reusable from Rust.

The clones used in the first session were in a scratchpad folder that is cleared on reboot.
Clone them again if needed.

## What exists (built, clippy clean, not yet committed)

```
Cargo.toml              workspace; edition 2024; GPL-3.0-or-later
crates/vst3-host/       onezan-vst3-host: the VST3 host
  module.rs             find the binary in the bundle, dlopen + ModuleEntry (Linux) / InitDll (Win),
                        GetPluginFactory, list classes, createInstance. Libraries are never unloaded.
  host.rs               HostApp (IHostApplication + IComponentHandler), HostMessage, HostAttributeList
  events.rs             EventList, ParamChanges / ParamQueue (pre-allocated, used on the audio thread)
  plugin.rs             Plugin (main thread): load, connect component and controller, start(sr, max_frames)
                        -> Processor (Send, audio thread): notes as events; CC, pressure and pitch
                        bend via the IMidiMapping table
  midi.rs               MidiEvent::parse (has a unit test)
  scan.rs               default_search_paths ($VST3_PATH, ~/.vst3, /usr/lib/vst3, ...), find_bundles, scan
crates/engine/          onezan-engine: midir input -> rtrb queues -> cpal output callback
                        (asks for a 256-frame buffer, f32); render_offline
apps/onezan/            CLI: list [--all] | midi | play [PLUGIN] [--midi PORT] [--demo] | render [PLUGIN]
README.md, LICENSE (GPL-3.0), .gitignore
```

`git init` has been run, but nothing is committed yet. Commit only when the user asks.

## Not yet verified

- **No sound has come out yet.** No VST3 plugin was installed. The user was installing
  `surge-xt` (`sudo pacman -Syu surge-xt`; at the `vst3-host` provider prompt the answer is **4 = carla**).
- No MIDI keyboard was connected. The only port was "Midi Through".
- Windows has never been built or tested.

## Next steps, in order

1. Check that Surge XT is installed: `ls /usr/lib/vst3`.
2. `cargo run -- list`, then `cargo run -- render "Surge XT"`. It must report a peak level, not "silence".
   If it crashes or is silent, debug against Carla, which can host the same plugin.
3. `cargo run -- play "Surge XT" --demo` to hear the scale. Then, with a keyboard,
   `cargo run -- play "Surge XT"`.
4. **Plugin editor window** (so the user can pick sounds): IPlugView, IPlugFrame, and IRunLoop
   on Linux (the plugin's timers and file descriptors must be serviced on the UI thread).
   Linux needs an X11 window, which runs under XWayland on Hyprland. Windows needs an HWND.
   transit's `view.rs` and `run_loop.rs` are the reference.
5. **State**: an IBStream MemoryStream; getState/setState on the component; sync the controller
   with setComponentState after loading; save the chosen plugin and its state to a config file.
6. A GUI (egui is the likely choice) with an on-screen keyboard showing the keys being played.
   This leads into the scale trainer.
7. Later: a metronome (generated inside the audio callback, sample-accurate), CLAP via
   `clack-host`, a built-in SoundFont fallback (`rustysynth`), the scale and fingering data, a
   learning order, and progress in SQLite (store raw note events).

## Known gaps and shortcuts in the current code

- All MIDI events take effect at the start of the audio block (sampleOffset 0). That adds
  jitter of up to one buffer (about 5 ms at 256 frames and 48 kHz). Acceptable for now.
- The controller is not synced with the component's state after loading (needs IBStream; step 5).
- `restartComponent` and `performEdit` from the plugin are ignored.
- Plugins run in our own process, so a plugin crash takes the app down.
- macOS module loading is not implemented (not a target).
- Scanning loads every bundle in-process, with no cache.

## Planning notes for the scale trainer (from the first session)

- An exercise is: scale (root and type) × hand (left / right / together / contrary motion)
  × octaves × direction × rhythm.
- Learning order: C, G, D, A, E major (same fingering); then B and F; then the black-key scales
  D♭, G♭ (and B), which are easier than they look; then B♭, E♭, A♭. Then harmonic minor in the
  same order, then melodic minor. Contrary motion, more octaves and faster rhythms come as levels
  on scales already learned. Allow free selection as well.
- Metronome and latency: timestamp on the audio thread; never use UI timers.

## Known issue: audio latency on Linux (2026-11-23)

Through cpal 0.16's ALSA backend on PipeWire, asking for a fixed buffer size (256, 512 and 1024
all tried) makes every `alsa::poll()` return POLLERR, and nothing plays. The engine now uses the
device's default buffer instead. That works, but callbacks are about 1880 frames (~43 ms at
44.1 kHz), which is too slow for playing live. `PIPEWIRE_LATENCY` does not change it. The
candidate fix is cpal's JACK host via pipewire-jack (installed): cpal's `jack` feature.

## Lints (2026-11-23)

Strict clippy, from https://www.namtao.com/rust/ ("Strict lints"): `[workspace.lints.clippy]` in the
root Cargo.toml (pedantic + nursery denied, plus the panic lints: unwrap/expect/indexing/
arithmetic/`as`/...), each crate opts in with `[lints] workspace = true`, and `clippy.toml` allows
unwrap/expect/panic/indexing in tests. `cargo clippy --all-targets` is clean on Linux.

- VST3 enum constants go through `util::venum` (checked `try_from`): they are `c_uint` on Linux
  and `c_int` on Windows, so `as` would be wrong on one of them. Counts go through `util::int32`.
- Two justified `#[expect]`s: `Module::load` keeps the cache locked on purpose
  (`significant_drop_tightening`), and `unsafe impl Send for Processor` (`non_send_fields_in_send_ty`).
- Also clean on Windows: `cargo clippy --target x86_64-pc-windows-gnu --all-targets` (checked
  only; nothing has run on Windows).
- `rustfmt.toml` sets `max_width = 120`; the code is formatted with it.
