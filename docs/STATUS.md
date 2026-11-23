# onezan: status and handoff

Last updated: 2026-11-23. Read this first when resuming.

## What onezan is

A desktop app for Linux and Windows. It reads a MIDI keyboard and plays it through a VST3
instrument. The long-term goal is a scale trainer: an on-screen keyboard with fingering numbers,
a scale picker, a suggested learning order, a metronome and progress tracking.

## Design rule: it just works

Most users start the program once; if it does not work, they delete it. So every setting has an
automatic default that picks the best option the system offers (output backend, buffer size,
sample rate, MIDI port, plugin), with no setup and no questions. Manual choices (input and
output device, and so on) come later, for people who know what they want or when the automatic
choice fails, but they are never required.

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

## What exists

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

Commit only when the user asks.

## Verified (2026-11-23)

- Surge XT is installed (`/usr/lib/vst3/Surge XT.vst3`); `list` finds it.
- `render` produces signal (peak about -9 dBFS, not silence).
- `play --demo`: the user heard the scale through Surge XT.

## Not yet verified

- Live playing beyond "notes sound": sustain pedal (CC 64), pitch bend, mod wheel. The keyboard
  (SWISSONIC EasyKeys49, passed through to the VM) works, but felt very sluggish on the old ALSA
  path; JACK has not been tried by ear yet.
- Anything running on Windows (it is only lint/type-checked).

## Next steps, in order

1. `cargo run -- play` with the keyboard on the JACK path: does it feel better? Check the sustain
   pedal (CC 64), pitch bend and the mod wheel.
2. Try the JACK path on the real (non-VM) machine and see what block size it gets.
3. Unit tests for the parts that can be tested without a plugin (they are thin now: one MIDI
   parse test). Candidates: `Callback::run` interleaving, `render_offline` event placement,
   `ParamChanges` and `EventList` capacity limits, `util` conversions.
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
  jitter of up to one buffer: about 5 ms at 256 frames, but ~43 ms with the current default
  buffer (see the latency issue below).
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

## Audio output and latency (2026-11-23)

The engine picks the output by itself (`pick_output` in `crates/engine`):

1. **JACK** on Linux, when a server answers. On PipeWire systems that is `pipewire-jack`: no
   jackd, nothing to set up. The app asks PipeWire for 256-frame blocks (`PIPEWIRE_LATENCY`, set
   at the start of `main` unless the user set it) and PipeWire grants that, or its configured
   minimum (`default.clock.min-quantum`) if that is larger. `libjack` is loaded at runtime, so
   the binary also starts without it.
2. Otherwise the platform default: **ALSA** on Linux, WASAPI on Windows. Always the backend's own
   buffer size: asking cpal 0.16's ALSA backend for a fixed size through PipeWire's ALSA plugin
   makes every poll return POLLERR, and nothing plays.

48 kHz is preferred (PipeWire's graph rate, so nothing is resampled).

**Startup report.** `play` (and `render`, for the plugin) prints what it uses for the plugin, MIDI
input and audio output, why, every alternative with the reason it was not used, and how to choose
differently. The block size is the one measured in the audio callback, with a note when the system
did not grant the size asked for. The selection code returns this as `Considered { name, verdict,
why }` lists (`onezan_engine::choose_midi_port`, `AudioOut::considered`), so a GUI can show the
same later. Logging defaults to `warn` so the report stays readable; `RUST_LOG=info` for details.

Measured in the dev VM (QEMU, emulated Intel HDA; its PipeWire has min-quantum 1024):
JACK 1024 frames = 21.3 ms per block; ALSA fallback 2048 frames = 42.7 ms. With JACK disabled
(`PIPEWIRE_NOJACK=1`) the fallback works. On the user's real machine latency is noticeably better.

Rough edges: the JACK client is called `cpal_client_out` (cpal 0.16 fixes the name), and when
JACK is unavailable pipewire-jack prints two warnings to stderr before the fallback.

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
