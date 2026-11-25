# onezan: status and handoff

Last updated: 2026-11-25. Read this first when resuming.

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
crates/vst3-host/       onezan-vst3-host: the VST3 host (no devices, no GUI)
  module.rs             find the binary in the bundle, dlopen + ModuleEntry (Linux) / InitDll (Win),
                        GetPluginFactory, list classes, createInstance. Libraries are never unloaded.
  host.rs               HostApp (IHostApplication + IComponentHandler), HostMessage, HostAttributeList
  events.rs             EventList, ParamChanges / ParamQueue (pre-allocated, used on the audio thread)
  plugin.rs             Plugin (main thread): load, connect component and controller, start(sr, max_frames)
                        -> Processor (Send, audio thread): notes as events; CC, pressure and pitch
                        bend via the IMidiMapping table. Plugin::editor() -> Editor
  editor.rs             Editor: the plugin's IPlugView in a parent window of the app; IPlugFrame
  run_loop.rs           Linux: IRunLoop, the editor's timers and descriptors serviced on the UI thread
  midi.rs               MidiEvent::parse (has a unit test)
  scan.rs               default_search_paths ($VST3_PATH, ~/.vst3, /usr/lib/vst3, ...), find_bundles, scan
  util.rs               conversions (platform enum types, strings, TUIDs)
crates/engine/          onezan-engine: MIDI in, audio out, timing
  lib.rs                module map, re-exports, MIDI queues (rtrb)
  instrument.rs         Instrument: a VST3 plugin or the built-in piano (rustysynth, a SoundFont);
                        Voice, its audio-thread half
  midi_in.rs            choose_midi_port, MidiWatch (opens, reconnects, releases held notes), Keys
                        (held keys with velocity, pedal; for display)
  audio_out.rs          pick_output (JACK, else default), request_low_latency, AudioOut
  callback.rs           the audio callback: drain queues, place events, run the plugin
  timing.rs             exact vs. immediate placement of live MIDI in a block
  metronome.rs          Metronome (settings, atomics), Clicker (clicks in the callback)
  report.rs             Considered / Verdict: what was considered and why (startup report)
  offline.rs            render_offline (checks without an audio device)
  tests/                plugin.rs (offline, against a real plugin), midi_hotplug.rs (virtual port)
apps/onezan/            no command (or `gui`): the window; CLI: list [--all] | midi |
                        play [PLUGIN] [--midi PORT] [--demo] | render [PLUGIN]
  main.rs               command line, play, render
  plugins.rs            installed plugins; resolve/load: which instrument an argument picks, and
                        why; the built-in piano when no plugin loads
  piano.rs              the built-in piano, embedded by build.rs (when assets/piano/ has it)
  session.rs            plugin + MIDI watcher + audio started together, with the report sections
  report.rs             the startup report as text
  gui.rs                the window (egui/eframe, OpenGL, our own event loop): editor, status bar,
                        keyboard
  embed.rs              the child window holding the plugin's editor (X11 on Linux, HWND on
                        Windows); fitting it in
  keyboard.rs           the on-screen keyboard widget: scale tint, velocity meter, fingering badges
  settings.rs           settings.json (config folder): tempo, click, scale, window size, the
                        instrument and keyboard used last time; data/config folder paths
  instrument_state.rs   the instrument's sound (plugin state) per plugin, saved on quit
  progress.rs           practice history: every note with its scale, in SQLite; practice time
  chords.rs             chord names (root, type, inversion, spelling) and a fingering for the held
                        keys; shown in the line above the keyboard
  fingering.rs          the fingering table: major, natural/harmonic/melodic minor, 12 keys, both
                        hands, any octaves; the preview the window shows
apps/onezan/build.rs    embeds assets/piano/*.sf2 when present (cfg onezan_piano), else warns
build/                  fetch-piano.sh (the built-in piano into assets/piano/, git-ignored);
                        Linux release build in a container (glibc 2.30+); Windows release zip
                        (windows-release.sh, cross-built with MinGW)
README.md, LICENSE (GPL-3.0), .gitignore
```

Commit only when the user asks.

## Verified (2026-11-23)

- Surge XT is installed (`/usr/lib/vst3/Surge XT.vst3`); `list` finds it; `render` produces signal.
- `play --demo` sounds. By hand with the keyboard (SWISSONIC EasyKeys49): notes, sustain pedal,
  pitch bend, fast repeated notes and big chords work; quitting with Enter and Ctrl+C is clean.
- The user's real machine (Linux Mint, PipeWire 1.0.5): ALSA gives 1280-frame blocks (26.7 ms),
  noticeable but okay; with pipewire-jack installed, `pw-jack onezan play` gets JACK at 256
  frames (5.3 ms), and auto timing picks exact. The dev VM gives 1024 frames on JACK.
- Offline checks against the installed plugin (`crates/engine/tests/plugin.rs`, skipped without
  one): a note at frame N sounds from N (not from its block start); a released note goes silent;
  the sustain pedal holds and releases.
- On the user's host (Mint), by hand: the window, the embedded Surge XT editor (works; the bar's
  menus and tooltips are now below it), velocity on the keys, the metronome with its options
  window (volume, three sounds), the Linux release build.

## Not yet verified / known problems

- MIDI reconnect: replugging the real keyboard verified by the user; also tested with a virtual ALSA port (`crates/engine/tests/midi_hotplug.rs`). Not
  tested on Windows.
- The mod wheel has no audible effect in Surge's default patch; unclear what it should do. Later.
- The octave button on a held note: set aside by the user for now.
- Exact MIDI timing has only been checked offline and by the report; no system here gives short
  enough blocks to hear it (it is picked at 11 ms or less).
- Real Windows: so far only under Wine (tests, plugin loading and processing with Surge XT, the
  embedded editor; see "Windows checks under Wine"). Audio devices, MIDI and latency are unchecked.
- Under Wine, after the window grows, a strip at its right edge can stay unpainted until the
  next repaint (seen once, growing a floating window; shrinking is fine; does not happen without
  an embedded editor). Probably Wine's OpenGL with child windows; check on real Windows.

## Next steps, in order

1. **Check by hand on the host**: settings, the instrument's sound and the practice history
   (all checked here in the VM, see "Saved between starts"); the scales window with the mouse.
2. **Scale trainer basics**: hands separately and contrary motion, choosing octaves, showing the way down
   (melodic minor differs), the learning order (`fingering::LEARNING_ORDER`). The table is done
   (see "Fingering table" below).
3. **Windows**: run the app on real Windows (so far only under Wine): audio, MIDI, the editor
   (embedded, see "The plugin editor").
4. **ASIO on Windows** (next milestone; see "ASIO (planned)" below for what is known).
5. A built-in sound when no VST3 instrument is installed: done (see "Built-in piano"); check
   by hand on the host, and the instruments window with the mouse.
6. Choosing the MIDI port, audio output and timing in the window: done (see "Choosing MIDI,
   audio and timing"); check by hand with the mouse, and on Windows.
7. Smaller, when convenient: compare `--midi-timing` modes by ear on the host; keyboard focus
   in the embedded editor; sharpness under fractional scaling (XWayland).
8. Later: CLAP via `clack-host`.

## Known gaps and shortcuts in the current code

- Live MIDI is stamped with `Instant::now()` when midir's callback runs, not with the driver's
  timestamp (midir's is on another clock, and differs per platform). Good to well under 1 ms.
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

**Debian, Ubuntu and Mint** keep PipeWire's `libjack` out of the library path
(`/usr/lib/<arch>/pipewire-0.3/jack/`; the location is a per-distro build option), so the app only
gets JACK when started through `pw-jack`. Verified on the user's Mint machine (PipeWire 1.0.5). By
the user's decision nothing is done automatically: when the output is not JACK but `pw-jack` is in
`$PATH`, the report's JACK line says to start with `pw-jack <the same command>`. (Considered and
not done: re-running through `pw-jack` automatically, or preloading the library from a guessed path.)

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

## MIDI timing (2026-11-23)

`crates/engine/src/timing.rs`. Each live event carries its arrival time. The audio callback maps
the interval since the previous callback onto the block (**exact**: notes keep their spacing, one
block later) or puts everything at frame 0 (**immediate**: no extra delay, 0 to 1 block late,
uneven). `--midi-timing auto` (default) picks exact for blocks of at most 11 ms, else immediate,
per callback, so it follows block size changes. The plugin gets each event at its sample
(`sampleOffset`, and the parameter point offset for CCs). Events from several sources are merged
with a stable insertion sort (no allocation). `play` reports the mode and why.

## MIDI reconnect (2026-11-23)

`crates/engine/src/midi_in.rs`. `MidiWatch` polls the port list once a second (midir has no
hotplug events). When the open port disappears, it sends note-offs for every key it held, and
sustain, mod wheel and pitch bend back to rest, so nothing hangs. When no port is open, it opens
the one `choose_midi_port` picks (so `--midi` applies to later keyboards too). `play` no longer
exits without a keyboard: it says to plug one in, and prints each connect and unplug. A failing
port is retried every poll and reported once. A replug within one poll that gets the same ALSA
port address back would go unnoticed.

## The window (2026-11-23)

`onezan` without a command opens it (`onezan gui [PLUGIN] [--midi PORT] [--midi-timing ...]` for
overrides). egui 0.36 through eframe with the OpenGL (glow) backend, not wgpu: lighter, and
works on any GL driver (the VM has only llvmpipe).

- **Layout, top to bottom**: the plugin's editor, the status bar, the keyboard. The editor is a
  native window over ours, so nothing egui draws can show over it (menus and tooltips vanished
  behind it on the user's host); with the bar below it they open over the keyboard instead.
- **Options window**: the "Options" button at the bar's right opens a window of its own (an egui
  immediate viewport, a real OS window: the plugin's editor would cover anything drawn inside
  the main window). It holds the metronome's volume and sound. It is repainted when the desktop
  resizes it (a tiling desktop does so at once; once it stayed white without that).
- **Status bar**: instrument and MIDI names cut to 24 characters with "…" (full names in the
  hover report), MIDI port (green dot when connected; "plug in a keyboard" otherwise;
  live), a pedal LED (amber while sustain is down; always there, so nothing moves), audio
  backend and block length, timing mode. Hovering an
  item shows the startup report for it (what was chosen, why, the alternatives). On the right,
  the fingering picker. When the window is narrow the left part is cut off, not overlapped.
- **Top**: the plugin's own editor (see "The plugin editor" below), at the top and centred
  across; a note when there is none.
- **Keyboard**: 49 keys, C2 to C6, by default. Reasons: the most common small keyboard size (the
  user's), and exactly two octaves per hand for scales hands together, left hand C2 to C4 and
  right hand C4 to C6, meeting on middle C. A played key outside the range widens it by whole
  octaves, so nothing played is invisible. MIDI cannot tell the keyboard's size, so this is the
  only way to adapt.
- **Key design** (`keyboard.rs`), in layers: key (white or black); state (in the scale: light
  tint; pressed: light blue tint plus a blue fill rising from the front edge like a level meter,
  as high as the key was struck hard: velocity 127 fills the key); up to two fingering badges, numbered circles, right hand blue and left
  hand orange. Badges sit where the finger lands: the front of a white key below the black keys,
  the lower end of a black key. When both hands use one key, the left badge is above the right.
  C keys are labelled with their octave (C4 = middle C).
- Held keys come from the engine: `onezan_engine::Keys` (per key the velocity it was struck with
  while held, 0 when up; all channels merged; and the sustain pedal), written by the MIDI thread, which also wakes the window to repaint.
- Fingering preview: the bar's "Scale" button opens the scales window (see "Saved between
  starts"); the chosen scale is shown two
  octaves hands together going up. The hands meet on the root nearest middle C (C major: left
  C2 to C4, right C4 to C6); a scale reaching past the keys widens the keyboard while shown.
- Checked in the VM with screenshots (`grim`), with notes sent through the Midi Through port
  (`onezan gui --midi through` plus `aseqsend -p 14:0 90 3C 64`).

## Linux release build (2026-11-23)

`build/linux-release.sh` builds in an Ubuntu 20.04 container (`build/linux.Dockerfile`), with
rootless Podman (preferred: no root, no daemon, no docker group; the user's choice) or Docker.
Ubuntu 20.04 was the user's choice for the widest reach without going ancient; it beats Debian
11 (same glibc) because its package archive stays online during extended support (to 2030).
Result: needs glibc 2.30+, links only libc/libm/libdl/libpthread/libgcc_s and libasound.so.2;
GL, X11/Wayland and JACK are loaded at runtime. Checked: the binary renders and opens the window
on Arch. First build about 2 minutes; crates are cached in `target/container-cargo-home`.

## The plugin editor (2026-11-23)

The middle of the window shows the plugin's own editor (`IPlugView`), embedded.

- **Host side** (`crates/vst3-host`): `Plugin::editor()` makes the view (`createView("editor")`,
  `X11EmbedWindowID` on Linux, `HWND` on Windows); `Editor::attach(parent)`, `size`, `can_resize`,
  `resize` (`checkSizeConstraint` + `onSize`), `requested_size` (from `IPlugFrame::resizeView`),
  `idle`. Dropping the editor calls `removed` and clears the frame; the plugin refuses to be torn
  down while its editor is alive (it leaks itself instead, like with a live processor).
- **Linux run loop** (`run_loop.rs`): the frame answers `IRunLoop`. Handlers and timers live on
  the UI thread; `service()` calls the handlers whose descriptor is readable (`poll` without
  waiting) and the due timers, never while holding a borrow (plugins register from inside).
  Surge XT registers two descriptors (its X11 connection and a timer descriptor) and no timers.
- **Window side** (`apps/onezan/src/embed.rs`, `gui.rs`): the window runs on X11 (winit
  `with_x11`; XWayland under Wayland). An X11 child window of ours (x11rb) is placed over the
  top area and the editor attached to it; a resizable editor is asked to fill the area (Surge keeps
  its proportions), any editor sits at the top, centred across, and is cut off where it does not
  fit. Verified working on the user's host (Mint). At the first
  frame the window asks to fit the editor (tiling desktops may ignore that).
- **Our own event loop**: eframe is driven with `create_native` + `pump_eframe_app`. While an
  editor is open the loop returns every 4 ms to service the plugin, without repainting the
  window. With eframe's own loop every service meant a repaint: 80 repaints a second and 300 %
  CPU in the VM (software GL); now about 4 % idle.
- **Closing**: the editor is removed when the window's close is requested, before the window and
  the plugin go. Checked: clean exit, nothing left registered.
- **Windows** (2026-11-23): a child `HWND` of ours (`WS_CHILD`, own window class) inside the
  main window, the editor attached to it; moved with `SetWindowPos`. The plugin's timers and input
  are window messages, dispatched by winit's loop on the UI thread, so `idle` does nothing there.
  winit gives the main window `WS_CLIPCHILDREN`, so our GL drawing leaves the editor alone.
  Surge XT (JUCE) asks with `resizeView` for its old size right after we resize it, yet stays at
  the new one (black band below it); so after a request the app takes `getSize`, not the rect.
  Checked under Wine with Surge XT 1.3.4 for Windows: embedded, fills the width, follows window
  resizes, hover tooltips show (mouse reaches it), clean exit on close.
- Not done or not checked: mouse and keyboard in the editor on Linux (the VM cannot send real
  input); under fractional scaling XWayland windows are scaled
  by the desktop and may look soft; keyboard focus inside the editor.

## Metronome (2026-11-23)

`crates/engine/src/metronome.rs`. The clicks are made inside the audio callback, after the
plugin, so every beat lands on its exact sample whatever the block size or UI load (never UI
timers). The beat position is kept as a fractional frame count, so odd tempos do not drift.
Every click is the same. Three sounds, each two decaying sines starting at their peak: Tick
(1500 Hz, bright, 12 ms), Woodblock (820 Hz, hollow knock), Beep (1000 Hz, longer, fades within
about 0.2 s so it fits 300 BPM).

- `Metronome` (shared, atomics): on/off, BPM (20 to 300), volume (0 to 1, linear; default 0.6),
  sound, and `play_sample()` (one click now, metronome on or off, to try a sound).
- Window: in the status bar's right part, a metronome button (a drawn metronome icon, highlighted while it runs) and a BPM field (drag, scroll or
  type). Volume and sound are in the options window; changing either plays a sample click. Space starts and stops it. On purpose nothing more: the user wants no beats per bar,
  accents or beat LEDs (an earlier version had them).
- `onezan play --metronome BPM` and `onezan gui --metronome BPM` start it at that tempo.
- Checked: unit tests (beats on their sample for blocks of 64 to 4096 frames, no drift at 70
  BPM, off/on), and recordings of the system output: clicks exactly 500.0 ms apart at 120 BPM
  and 666.7 ms at 90, all the same level. A unit test checks the volume scales the click.
- Not yet: tempo and volume are not saved between starts.

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

## Fingering table (2026-11-23)

`apps/onezan/src/fingering.rs`. Major, natural, harmonic and melodic minor in all twelve keys,
both hands, for any number of octaves, the traditional fingering. Each hand is stored as one
finger per scale degree plus the fingers that differ at the ends (the left hand's 5 on the bottom
root, the right hand's 5 on the top one, a different start finger). Melodic minor comes down as
natural minor, with its fingering.

Cross-checked against five sources; the tests hold their numbers verbatim and fail if the table
drifts. Every place we differ from one is listed in the test with the reason:

- pianoscales.org: major, natural and harmonic minor, one octave, all keys. All agree except
  G# harmonic minor's right hand start (it says 2 3; we use 3 4 like the next three sources).
- Hoffman Academy, Melodic Minor Scale Fingering Guide (PDF, two octaves, all keys). All agree
  except two end fingers: F# left hand top note (2; we keep 4 like the one-octave charts) and
  Bb right hand start (4; we use 2 like pianoscales.org and Greg Spero).
- Greg Spero (composition.gregspero.com/lessons/a3): natural minor and the melodic minor
  exceptions. All agree except C# melodic minor's right hand start (3 4; we use 2 3 like
  Hoffman).
- musicandhealth.co.uk: B, F# and C# major, two octaves. Agree except the left hand's top note
  of F# and Db (2; we keep 4 and 3).
- University of Miami chart (via cs.miami.edu/~burt): where the fourth finger falls in each key
  and type. All agree.

Settled by the sources against my first draft: G# harmonic minor's left hand is 3 2 1 4 3 2 1 3
(thumb on F##), not natural minor's 3 2 1 3 2 1 4 3. Hoffman's blog says natural and harmonic
minor share one fingering everywhere; three charts disagree for G#, so we follow them. Not
checked against the ABRSM books (not online).

A test also checks every scale, both directions, 1 to 4 octaves: the thumb only on white keys,
and the fingers only move by passing the thumb under (right hand up) or crossing over it.

## Saved between starts (2026-11-23)

Everything is best effort: what is missing, damaged or no longer works gives way to the
automatic choice or the default, and the app always starts. Checked in the VM with each case
made on purpose; the startup report's plugin line says what happened.

- **Settings** (`settings.rs`): `~/.config/onezan/settings.json` (`%APPDATA%\onezan` on Windows),
  JSON by the user's choice (`serde_json`). Tempo, click volume and sound, the scale shown, window
  size, the instrument (plugin class id) and MIDI keyboard (port name without its ALSA address)
  used last time. Every key optional, unknown keys ignored; an unreadable file is moved to
  `settings.json.broken`. Saved one second after a change (a drag saves once) and on quit,
  written to a temporary file and renamed. `--metronome BPM` wins over the saved tempo; a plugin
  or `--midi` given on the command line wins over the remembered ones. A saved window size turns
  off fitting the window to the editor at the first frame. `play` (no window) uses no settings
  except the instrument's sound.
- **Fallbacks**: a remembered instrument that fails to load → the first instrument. A remembered
  keyboard that is not plugged in → the automatic choice (not waited for). Audio output has no
  choice yet, so nothing is saved for it.
- **Instrument's sound** (`instrument_state.rs`, `Plugin::state`/`set_state`, `stream.rs`):
  `~/.local/share/onezan/instruments/<class id>.state`, the component's and controller's state.
  Saved on quit (window only), restored right after loading, in the SDK hosts' order (component,
  `setComponentState`, controller). Loading also gives the controller the component's state now.
  A damaged or refused file is moved aside (`.broken`, `.refused`); a marker file around the
  restore makes the next start skip a state the plugin crashed on (`.crashed`). Test: Surge XT's
  patch renamed in the saved bytes comes back after restoring (`crates/engine/tests/plugin.rs`).
- **Practice history** (`progress.rs`): `~/.local/share/onezan/progress.sqlite` (`rusqlite`,
  bundled SQLite), table `notes(t_ms, key, velocity, scale)`, every note press and release with
  the scale selected then (NULL: none). Fed by `Keys::set_note_listener` on the MIDI thread; a
  thread writes batches every 2 s, on a scale change and on quit (the quit waits for it). WAL, so
  the window reads while it writes. Practice time = gaps between presses added up, gaps over 15 s
  (`PAUSE`) left out; worked out when read, so it can change later. A damaged file is moved
  aside and a new one started.
- **Scales window**: the bar's "Scale: ..." button opens it (an OS window of its own, like the
  options). All 48 scales with time spent and last played, ordered by learning order or by name
  (alphabetical); click to show one, "No scale" to clear. A star per row marks favorites: they are
  listed first, under "Favorites", and kept in settings.json as `favorite_scales` (by name). Re-reads the history every 3 s while
  open.
- **Buttons**: every button has a lighter fill and a visible border (brighter on hover, darker
  pressed); toggles (Metronome, Scale, Options, the order) are real buttons that stay highlighted
  while on (`toggle` in `gui.rs`), not egui's frameless `toggle_value`.

## Windows checks under Wine (2026-11-23)

`.cargo/config.toml` makes Wine the runner for `x86_64-pc-windows-gnu`, with its prefix in
`target/wine-prefix` (not `~/.wine`). Needs `wine` and `mingw-w64-gcc` (Arch packages).

    cargo test --target x86_64-pc-windows-gnu --workspace   # about a minute the first time

All tests pass as Windows binaries. Linux-only ones are left out (the editor's run loop, the
virtual MIDI port). Surge XT 1.3.4 for Windows is installed in the prefix (its official installer,
run with `/VERYSILENT`), so the plugin tests run there for real: Windows module loading
(`InitDll`, bundle layout), processing, the sustain pedal and restoring a saved state all pass.
Without it in the prefix (a fresh `target/`), they skip.

Found this way: on Windows the class id is kept in COM's GUID byte order, so the same plugin
printed a different id than on Linux. Ids are now printed in the SDK's canonical order
everywhere (`util::class_id_string`), as other hosts and `.vstpreset` files show them. A test checks the
settings and data folders are `%APPDATA%\onezan` there. The app itself runs under Wine
(`wine target/x86_64-pc-windows-gnu/debug/onezan.exe`): the window, the Windows plugin folders,
no MIDI ports (Wine passes none through by default), and the plugin's editor embedded (see
"The plugin editor"). Wine is not Windows: real audio, MIDI,
latency and the editor still need a Windows VM or PC (see the next steps).

## Choosing the instrument (2026-11-23)

The bar's "Instrument: ..." button opens the instruments window (an OS window of its own): the
installed VST3 instruments, the one in use highlighted, the bundle path on hover. They are
scanned when the window first opens; "Rescan" looks again (after installing one). Clicking one
switches (`App::switch_instrument`): the current sound is saved, the editor removed, the session
(plugin, MIDI input, audio) stopped, then a new session started with that plugin through the
same code as at startup (`start_session`), with the keyboard in use, the metronome's settings and
whether it was running, and the scale. The new instrument's own saved sound is restored, and it
is remembered for the next start. If it fails, the previous instrument is started again and the
window says why in red. The window keeps its size (the new editor is fitted into it).

**Favorites.** Each row has a star (☆/★). Starred instruments (the built-in piano too) are listed
first, under "Favorites", then "All instruments" with the rest. The class ids are kept in
settings.json as `favorite_instruments`, in the order starred; a starred plugin that is uninstalled is simply
not listed (its id stays, so it comes back if reinstalled). Not checked with the mouse yet.

Checked in the VM with a throwaway build that switched by itself (no click tool here): a switch
to Surge XT again (full teardown and restart: sound saved and restored, editor embedded again,
MIDI reconnected) and to a missing id (Surge XT back, the reason shown). Not checked: switching
between two different instruments (only one is installed here), with the mouse.

## Choosing MIDI, audio and timing (2026-11-23)

The bar's MIDI, Audio and Timing values are buttons that open menus (egui popups, opening
downwards over the keyboard: the editor would cover anything above; long lists scroll). Each
menu starts with "Automatic", which stays the default; hovering still shows the report.

- **MIDI**: Automatic (the first port that is not a loopback) or a port, listed when the menu
  opens. `MidiWatch::choose` closes the open port (held notes released) and opens the one the
  query picks at once; the query stays for later replugs. The connected port is remembered as
  before.
- **Audio**: Automatic (`pick_output`) or any device of any audio system
  (`onezan_engine::audio_outputs`, listed when the menu opens; ALSA's listing prints "Unknown PCM"
  lines to stderr). JACK is one entry, "default": cpal names its devices after our own client
  (`cpal_client_out-62`), which changes every run. Choosing one restarts only the audio stream
  (`Session::set_output`): the plugin is deactivated and started again, its editor stays open, the
  keyboard gets a new queue (`MidiWatch::set_queue`). An output that fails gives way to the
  automatic choice, and the menu says why in red. Saved as `"audio": {"backend", "device"}`; a
  saved output that is gone falls back to automatic at start (logged).
- **Timing**: auto, exact or immediate, applied from the next audio block (`SharedTiming`, an
  atomic the callback reads). Saved as `"midi_timing"`; `--midi-timing` on `gui` wins over it.
- Switching instrument keeps all three.

Checked on the host with a throwaway build that picked by itself (no click tool): timing changed
live; ALSA "default" chosen (block 2048) and back to JACK; the Midi Through port chosen and back
to automatic (released, waiting for a keyboard); a missing output fell back to automatic; the
choices saved and used at the next start; the audio menu open over the keyboard (screenshot). Test:
`midi_hotplug.rs` switches between two virtual ports (held note released, new queue used). Not
checked: clicking with the mouse, the menus on Windows/Wine.

## ASIO (planned, next milestone)

The low-latency drivers of audio interfaces; Windows users expect it in the Audio menu.
DirectSound is not planned: since Vista it runs on WASAPI, so it is never faster, and cpal does
not have it.

- **Plan**: cpal's `asio` feature for Windows only. ASIO drivers appear in the Audio menu by
  themselves (cpal lists the ASIO host). Automatic stays WASAPI at first: generic drivers
  (ASIO4ALL, FlexASIO, Realtek's) can take the device away from other programs or misbehave.
  Later, once checked on real hardware, Automatic could prefer an interface's own ASIO driver.
- **Licence**: the ASIO SDK is dual-licensed, proprietary or GPLv3 (since October 2025; its
  LICENSE.txt says so). onezan is GPL-3.0-or-later, so it fits. asio-sys downloads the SDK at
  build time (to `/tmp/asio_sdk`). asio-sys itself is Apache-2.0.
- **Building for `x86_64-pc-windows-gnu` from Linux** (tried 2026-11-23, then taken out again):
  needs clang. Two problems, both solved:
  1. asio-sys does not tell bindgen the target, so clang lays structs out for Linux. Fix, in
     `.cargo/config.toml` `[env]`: `BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_gnu =
     "--target=x86_64-w64-mingw32 -I/usr/x86_64-w64-mingw32/include"` (then
     `cargo clean -p asio-sys --target x86_64-pc-windows-gnu`: the build script does not rerun).
  2. Still a layout check failure: `AsioDrivers::connID` at offset 12. MinGW uses the Itanium C++
     ABI, which puts a derived class's first field in the base class's tail padding; a `repr(C)`
     struct cannot. Rust never uses its fields. Fix: a vendored asio-sys 0.2.6 with
     `.opaque_type("AsioDrivers")` after `.allowlist_type("AsioDrivers")` in `build.rs`, through
     `[patch.crates-io]` (and `exclude` it from the workspace). asio-sys 0.4 (newest cpal) has the
     same allowlist, so upgrading cpal does not help. Worth sending upstream.
  With both, the Windows build succeeded.
- **Testing under Wine**: wineasio 1.3.0 (Arch package) with Wine 11.20. Its `wineasio-register`
  script fails ("no wine64 was found": Wine is a single `wine` binary now), and
  `wine regsvr32 wineasio64.dll` fails too ("Failed to load DLL", status c0000135: a DLL it
  needs was not found). Unsolved; next idea: check what `wineasio64.dll.so` needs (libjack, with
  JACK running through PipeWire) and whether the DLL must be copied into the prefix's
  `system32`. Real latency and real interface drivers need a Windows PC either way.

## Windows: a GUI program (2026-11-23)

`#![windows_subsystem = "windows"]`: started from Explorer or a shortcut, no console window
opens. `src/console.rs`: started from a terminal, the app attaches to it (`AttachConsole`), so
the report and the command-line commands print there; a command-line command with nowhere to
write (started from Explorer) opens a console of its own; redirected output (`> file`) is left
alone. A GUI program does not make cmd/PowerShell wait: the prompt comes back at once, and the
terminal competes for the keyboard (`play`'s "press Enter"; Ctrl+C works). Checked under Wine:
subsystem 2 (GUI) in the PE header; `list` into the terminal and into a file; `wine start
onezan.exe` opens only the window; `wine start onezan.exe play --demo` opens a console with the
report.

## Playing the on-screen keyboard (2026-11-23)

Pressing a key with the mouse plays it at velocity 127; releasing the button, or sliding to
another key (a glissando), releases it. `keyboard::show` returns the key under the held pointer
(black keys lie over the white ones; unit test `the_mouse_finds_the_key_under_it`); the session
has a queue of its own for it (`Session::play`), made again when the audio output changes. The
key shows pressed at full velocity. Not recorded in the practice history (that is for the MIDI
keyboard). Checked by hand on the host (Linux).

The plugin tests (`crates/engine/tests/plugin.rs`) now run one at a time: with Dexed installed
(it sorts before Surge XT, so it became the test plugin) four instances on parallel threads
hung. The app only ever uses plugins from one thread.

## Windows release zip (2026-11-23)

`build/windows-release.sh [DIR]` cross-builds with MinGW (no container: the exe links only to
Windows' own DLLs, including the Universal C Runtime, `api-ms-win-crt-*`, part of Windows 10+),
strips it (27 to 20 MB), and zips it with README.md and LICENSE as
`target/windows-release/onezan-VERSION-COMMIT-windows-x64.zip` (about 8 MB); with DIR it copies
the zip there. It fails if the exe would need a MinGW runtime DLL (libgcc, libstdc++,
libwinpthread). Uncommitted changes make the name `COMMIT-dirty`. The zip from 49f2cb3 opened
its window and listed Surge XT under Wine; not run on real Windows yet.

## Chords (2026-11-25)

`apps/onezan/src/chords.rs`. A line between the bar and the keyboard names what is held: the
chord's symbol (C/E), its name (C major, first inversion) and its notes as played, spelled from
the root (E G# B, not E Ab B). Keys that make no chord show their names, and for two notes the
interval. Once the keys are up the last chord stays, dimmed. A chord held gets fingering badges on
its keys where the sources agree (below), in place of the scale's on those keys. Chord badges
are outlined (white circle, the hand's colour for ring and number), scale badges filled, so the
two stay apart while a scale is shown.

- **Naming**: every held pitch class is tried as the root against 20 chord types (major, minor,
  dim, aug, sus2, sus4, 7, maj7, m7, m7b5, dim7, 6, m6, mMaj7, 7sus4, add9, madd9, 9, maj9, m9).
  Sevenths and ninths also match without their fifth. Ties: the root in the bass wins (A C E G is
  Am7, C E G A is C6), then the full chord, then the simpler type. A bass other than the root
  gives a slash chord, named by its inversion.
- **Hands**: which hand plays is not guessed. Keys within an octave get both hands' fingerings
  (where agreed), stacked like the scale badges; wider, they are split at the biggest gap each
  hand can span, the lower part left hand. A lone bass note or an interval gets no badge.
- Checked in the VM through Midi Through (first version, before the table below): the chord
  line, the dimmed last chord, badges. Not checked by hand on a real keyboard.

## Chord fingering (2026-11-25)

A table, not a rule, and deliberately incomplete: **a fingering is shown only where at least
two sources give one and none gives another.** There is no official standard for block chords:
the exam boards' books (ABRSM, Trinity, RCM) print broken chords and arpeggios, and teachers
differ. Covered: major and minor triads in close position, and the same with the bottom note
doubled an octave up (C E G C), each inversion. Nothing for diminished, augmented, seventh or
other chords, wider voicings, or a hand holding one note or an interval. The tests hold the
sources' numbers verbatim and work out from them what may be shown, so the table cannot drift
from them.

- "Piano Chords in Inversions", appendix 16.2 of a Salvation Army music manual
  (music.saconnects.org, an engraved score; read from the rendered pages): triads and four-note
  chords in all 24 keys, both hands, the same fingering in every key. Triads RH 135 125 124, LH
  531 531 521 (minor: 531 421 521); four notes RH 1235 1245 1235, LH 5321 5421 5321.
- musicandhealth.co.uk, a teacher's ABRSM-style syllabus: four-note broken chords key by key
  (C, D, A, E, Eb, Ab, Db, B, Bb, F# major; F, C, G, F#, C#, G#, B, Bb, Eb minor). RH 1235 1245,
  then 1245 (major) or 1235 (minor); the LH depends on the key: root position 5421 in C and all
  minors, 5321 in the other majors. Where it offers an alternative, its first choice counts.
- Hoffman Academy, "Chord Inversions" (chart and text: "all fingerings here use the third finger
  except first inversion on the right hand and second inversion on the left, which use finger
  2") and Piano with Lauren's chart: triads RH 135 125 135, LH 531 531 521.
- Yamaha, "Basic piano chords for beginners": major and minor root position 1 3 5.

Shown: triads RH root 135, 1st 125; LH root 531, 1st 531 (major only), 2nd 521. Four notes RH
root 1235, 1st 1245, 2nd 1235 (minor only); LH 1st 5421 (not D, A, E, F#, C#, G# minor), 2nd
5321, root 5321 (major, not C, F, G).

Left out because the sources differ: RH second inversion triad (135 Hoffman and Lauren / 124
Salvation Army; Strum&Key and Deviant Noise list both); LH first inversion minor triad (531 /
421); four notes RH second inversion major (1235 / 1245); LH root position minor (5321 / 5421)
and C major (5321 / 5421); LH first inversion F#, C#, G# minor (5421 / 5321). A key
musicandhealth does not cover counts only where it gives the same fingering in every key it
covers (so F and G major's LH root position, which varies there, is left out).

Also read and not used: Strum&Key and Deviant Noise (commercial pages; their triads agree with
Hoffman, with 124 as an alternative); seventh chords from Skoove (Cmaj7), vocalrangetest (C7
root only) and musicandhealth's dominant seventh broken chord (Eb7): different chord types, one
source each, and they disagree on inversions. Not reachable: the RCM and ABRSM books (not
online), Wheaton College's arpeggio handout and an Ontario open textbook (both refused the
download).

## Built-in piano (2026-11-25)

So the first start always makes sound. FreePats "Upright Piano KW" (2022-02-21), a Kawai upright,
SF2, stereo, two velocity layers, CC0 (public domain); played by `rustysynth` 1.3 (pure Rust,
MIT). The user compared three renders of the same phrase (this one in its small 10 MB version,
this one, and FreePats' YDP Grand, 118 MB, CC-BY) and chose this one.

- **Embedded, uncompressed** (the user's choice over zstd-compressed, 35 MB, unpacked in 0.2 s):
  one binary, nothing beside it. `build/fetch-piano.sh` downloads the official archive (checked
  against a pinned SHA-256), unpacks it (bsdtar or 7z) into `assets/piano/` (git-ignored, with its
  cc0.txt and readme.txt); `apps/onezan/build.rs` then embeds it with `include_bytes!` (cfg
  `onezan_piano`). Without it the program builds with a warning and has no built-in piano. Both
  release scripts fetch it first. Sizes: Windows exe 20 to 77 MB, zip 8 to 51 MB; Linux release
  binary 77 MB. Parsed the first time it is used (57 MB in memory then), once per run.
- **Engine**: `Instrument` (a `Plugin` or the `Piano`) replaces `Plugin` in `AudioOut::start`,
  `render_offline` and the session; `Voice` (a plugin `Processor` or a `PianoVoice`) in the
  callback. The piano renders up to each event's frame, so every event plays on its own sample,
  like a plugin. Notes, all controllers (sustain) and pitch bend; pressure is ignored. rustysynth's
  own reverb and chorus are on (its default; the renders the user liked had them).
- **When it plays**: `play built-in` (or its id `built-in-piano`), the instruments window (listed
  first), or the remembered choice. And as a stand-in when nothing was asked for and no plugin
  loads (none installed, or the first one fails): the report says why, the top area says so,
  and the stand-in is not saved as the choice (`Session::is_fallback`), so a plugin installed
  later is used. A plugin asked for by name that fails is still an error. No sound is saved for
  it (no state). `list` shows it.
- Checked: engine tests (`tests/piano.rs`: a note sounds from its frame, a released note dies
  away, the pedal holds then releases; skipped without the file); with all plugin folders hidden
  (`bwrap --tmpfs /usr/lib/vst3 --tmpfs ~/.vst3`): `play --demo` falls back and is heard (system
  output recorded with `pw-record`), the window shows it, and the settings keep no instrument;
  `render built-in` on Linux and under Wine (the Windows release exe); the Linux container build;
  building without the file (warning), and with it again (the folder is watched too, as a file
  put back keeps its old time). Not checked: by hand on the host, choosing it with the mouse.

## Testing (2026-11-25)

`docs/TESTING.md`: the plan (contracts between modules, not insides) and what was done. Run
`build/check.sh` before a commit: format, clippy and tests for Linux and, under Wine, Windows
(`--quick`: Linux only). Saved-file fixtures in `apps/onezan/fixtures/` and
`crates/vst3-host/fixtures/` must keep loading. 97 tests on Linux at this point.
