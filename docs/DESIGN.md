# onezan: how it works

The reference for how each part of onezan works and why it is built that way. Where things
stand and what comes next is in STATUS.md; what is checked by hand before a release is in
RELEASE.md.

## Design rule: it just works

Most users start the program once; if it does not work, they delete it. So every setting has an
automatic default that picks the best option the system offers (output backend, buffer size,
sample rate, MIDI port, plugin), with no setup and no questions. Manual choices (input and
output device, and so on) are there for people who know what they want or when the automatic
choice fails, but they are never required.

## Decisions

| Topic | Decision |
|---|---|
| Language | Rust. JUCE/C++ was considered and rejected. |
| License | Open source and copyleft-compatible. The app is **GPL-3.0-or-later**: Apache-2.0 dependencies are compatible with GPLv3, not with GPLv2-only. |
| Plugin hosting | **Option C:** our own thin host on the `vst3` bindings crate (coupler-rs, v0.3), behind our own API. |
| Rejected as host libraries | `vst3-host` (one maintainer, little Linux testing with real plugins), `maolan-plugin-host` (too new), `rack` (macOS-first, no commits since Nov 2025), `plughost` (no Linux). |
| Fingering | MIDI cannot tell which finger was used. The app shows fingering but cannot check it. Fingerings come from hand-written data tables checked against published sources, not from an algorithm. |

## The code

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
  scan.rs               default_search_paths ($VST3_PATH, ~/.vst3, /usr/lib/vst3, ...), find_bundles,
                        scan / scan_bundle, fingerprint (a bundle's binary: size and mtime)
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
  known_plugins.rs      plugins.json (data folder): the scan cache, and the plugins that crashed
                        onezan (a note written around each scan, load and editor opening)
  piano.rs              the built-in piano, embedded by build.rs (when assets/piano/ has it)
  session.rs            plugin + MIDI watcher + audio started together, with the report sections
  report.rs             the startup report as text
  gui.rs                the window (egui/eframe, OpenGL, our own event loop): editor, status bar,
                        keyboard; a message box when the window cannot open (no OpenGL 2.0)
  last_error.rs         the logger (env_logger), keeping the last error: eframe only logs why the
                        window could not open
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
  waterfall.rs          falling notes: Song (a scale, or a MIDI file via
                        midly), Run (play along with the metronome, or wait), drawing
  staff.rs              music notation with Bravura (assets/fonts/, SIL OFL 1.1, embedded
                        unchanged: a subset may not keep the name): the clef and key signature
                        in each row of the scales window
apps/onezan/build.rs    embeds assets/piano/*.sf2 when present (cfg onezan_piano), else warns;
                        on Windows (MinGW) compiles the exe's icon in
build/                  fetch-piano.sh (the built-in piano into assets/piano/, git-ignored);
                        Linux release build in a container (glibc 2.30+); Windows release zip
                        (windows-release.sh, cross-built with MinGW); check.sh (before a commit);
                        make-icon.sh (the icons from assets/icon/onezan.svg)
README.md (for users), LICENSE (GPL-3.0); docs/: STATUS.md (where things stand), DESIGN.md (this),
RELEASE.md (hand checks and release steps), TESTING.md (the test plan), BUILDING.md
```

## The window

`onezan` without a command opens it (`onezan gui [PLUGIN] [--midi PORT] [--midi-timing ...]` for
overrides). egui 0.36 through eframe with the OpenGL (glow) backend, not wgpu: lighter, and
works on any GL driver (the VM has only llvmpipe).

- **Layout, top to bottom**: the plugin's editor, the status bar, the keyboard. The editor is a
  native window over ours, so nothing egui draws can show over it (menus and tooltips vanished
  behind it on the user's host); with the bar below it they open over the keyboard instead.
- **Options window**: the "Options" button at the bar's right opens a window of its own (an egui
  immediate viewport, a real OS window: the plugin's editor would cover anything drawn inside
  the main window). It holds the audio output, the MIDI timing, the metronome's volume and sound,
  and the theme. It is repainted when the desktop resizes it (a tiling desktop does so at once;
  once it stayed white without that).
- **Status bar**, left to right: the instrument (opens the instruments window), the MIDI port
  (green dot when connected, "plug in a keyboard" otherwise; a menu), a pedal light (amber while
  sustain is down; always there, so nothing moves; a click latches the pedal down, the next click
  or a release of the real pedal lets go: `MousePedal`, sent on channel 0 like the mouse's
  notes), and the mode button (Learn Song / Edit instrument, see "Falling notes"). On the right:
  Options, Scale, and the metronome (button, BPM, Tap). Instrument and MIDI names are cut to 24
  characters with "…". Hovering an item shows the startup report for it (what was chosen, why,
  the alternatives). When the window is narrow the left part is cut off, not overlapped.
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

## The plugin editor

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
- Under fractional scaling, XWayland windows are scaled by the desktop and may look soft.
  Keyboard focus inside the editor is not handled.

## Audio output and latency

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
why }` lists (`onezan_engine::choose_midi_port`, `AudioOut::considered`); the window shows the
same when hovering the bar's items. Logging defaults to `warn` so the report stays readable; `RUST_LOG=info` for details.

Measured on the user's machine (Linux Mint, PipeWire 1.0.5): ALSA gives 1280-frame blocks
(26.7 ms), noticeable but okay; through `pw-jack`, JACK at 256 frames (5.3 ms), and auto timing
picks exact.

Measured in the dev VM (QEMU, emulated Intel HDA; its PipeWire has min-quantum 1024):
JACK 1024 frames = 21.3 ms per block; ALSA fallback 2048 frames = 42.7 ms. With JACK disabled
(`PIPEWIRE_NOJACK=1`) the fallback works. On the user's real machine latency is noticeably better.

Rough edges: the JACK client is called `cpal_client_out` (cpal 0.16 fixes the name), and when
JACK is unavailable pipewire-jack prints two warnings to stderr before the fallback.

## MIDI timing

`crates/engine/src/timing.rs`. Each live event carries its arrival time. The audio callback maps
the interval since the previous callback onto the block (**exact**: notes keep their spacing, one
block later) or puts everything at frame 0 (**immediate**: no extra delay, 0 to 1 block late,
uneven). `--midi-timing auto` (default) picks exact for blocks of at most 11 ms, else immediate,
per callback, so it follows block size changes. The plugin gets each event at its sample
(`sampleOffset`, and the parameter point offset for CCs). Events from several sources are merged
with a stable insertion sort (no allocation). `play` reports the mode and why.

## MIDI reconnect

`crates/engine/src/midi_in.rs`. `MidiWatch` polls the port list once a second (midir has no
hotplug events). When the open port disappears, it sends note-offs for every key it held, and
sustain, mod wheel and pitch bend back to rest, so nothing hangs. When no port is open, it opens
the one `choose_midi_port` picks (so `--midi` applies to later keyboards too). `play` no longer
exits without a keyboard: it says to plug one in, and prints each connect and unplug. A failing
port is retried every poll and reported once. A replug within one poll that gets the same ALSA
port address back would go unnoticed.

## Choosing the instrument

The bar's "Instrument: ..." button opens the instruments window (an OS window of its own): the
installed VST3 instruments, the one in use highlighted, the bundle path on hover. They are
scanned when the window first opens; "Rescan" looks again (after installing one). Clicking one
switches (`App::switch_instrument`): the current sound is saved, the editor removed, the session
(plugin, MIDI input, audio) stopped, then a new session started with that plugin through the
same code as at startup (`start_session`), with the keyboard in use, the metronome's settings and
whether it was running, and the scale. The new instrument's own saved sound is restored, and it
is remembered for the next start. If it fails, the previous instrument is started again and the
window says why in red.

**Plugins that crash onezan** (`known_plugins.rs`). Plugins run in our process, so one that
crashes while it is scanned, loaded or opens its editor takes onezan down, and would again at
every start (the remembered instrument is loaded first). Around each of these, the bundle and
what is being done are written to `plugins.busy` in the data folder, and removed after. A note
still there at the next start (taken in once per run, before anything is scanned or loaded) is
recorded in `plugins.json` with the bundle's fingerprint. From then on that bundle is skipped:
one that crashed while it was scanned is not scanned (the instruments window lists it under
"Skipped", with "Try again"); one that crashed later is still listed, greyed out, but is never
picked automatically nor restored as the one used last time (the report says why). Asking for it
(a click in the window, or a name or path on the command line) tries it again; so does an update
(a new fingerprint). Not covered: a crash while it plays (audio thread) or later in its editor.
Two onezans scanning at the same moment could take each other's note for a crash. Checked by hand
with `build/test-plugins/` (two bundles that crash on purpose, see QA.md).

**Scan cache.** `plugins.json` also keeps what each bundle held (name, vendor, class id,
sub-categories) with its fingerprint, so a start loads only new or updated bundles. Bundles that
fail to load are not kept (they are tried again at each scan: a missing library may have been
installed). A file from another version (`version`) is scanned afresh, keeping its crashes; a
damaged file is started afresh. The window keeps its size (the new editor is fitted into it).

**Favorites.** Each row has a star (☆/★). Starred instruments (the built-in piano too) are listed
first, under "Favorites", then "All instruments" with the rest. The class ids are kept in
settings.json as `favorite_instruments`, in the order starred; a starred plugin that is uninstalled is simply
not listed (its id stays, so it comes back if reinstalled).

Checked in the VM with a throwaway build that switched by itself (no click tool here): a switch
to Surge XT again (full teardown and restart: sound saved and restored, editor embedded again,
MIDI reconnected) and to a missing id (Surge XT back, the reason shown).

## Choosing MIDI, audio and timing

MIDI is a menu in the bar (an egui popup, opening downwards over the keyboard: the editor would
cover anything above; long lists scroll). Audio and timing are lists in the Options window,
under "Audio output" and "MIDI timing", with what is in use (the report on hover); the outputs
are listed when Options opens. Each starts with "Automatic", which stays the default.

- **MIDI**: Automatic (the first port that is not a loopback) or a port, listed when the menu
  opens. `MidiWatch::choose` closes the open port (held notes released) and opens the one the
  query picks at once; the query stays for later replugs. The connected port is remembered as
  before.
- **Audio**: Automatic (`pick_output`) or any device of any audio system
  (`onezan_engine::audio_outputs`, listed when Options opens; ALSA's listing prints "Unknown PCM"
  lines to stderr). JACK is one entry, "default": cpal names its devices after our own client
  (`cpal_client_out-62`), which changes every run. Choosing one restarts only the audio stream
  (`Session::set_output`): the plugin is deactivated and started again, its editor stays open, the
  keyboard gets a new queue (`MidiWatch::set_queue`). An output that fails gives way to the
  automatic choice, and the list says why in red. Saved as `"audio": {"backend", "device"}`; a
  saved output that is gone falls back to automatic at start (logged).
- **Timing**: auto, exact or immediate, applied from the next audio block (`SharedTiming`, an
  atomic the callback reads). Saved as `"midi_timing"`; `--midi-timing` on `gui` wins over it.
- Switching instrument keeps all three.

Checked on the host with a throwaway build that picked by itself (no click tool): timing changed
live; ALSA "default" chosen (block 2048) and back to JACK; the Midi Through port chosen and back
to automatic (released, waiting for a keyboard); a missing output fell back to automatic; the
choices saved and used at the next start; the audio menu open over the keyboard (screenshot). Test:
`midi_hotplug.rs` switches between two virtual ports (held note released, new queue used). Since then
checked by hand with the mouse on Linux and real Windows.

## Playing the on-screen keyboard

Pressing a key with the mouse plays it at velocity 127; releasing the button, or sliding to
another key (a glissando), releases it. `keyboard::show` returns the key under the held pointer
(black keys lie over the white ones; unit test `the_mouse_finds_the_key_under_it`); the session
has a queue of its own for it (`Session::play`), made again when the audio output changes. The
key shows pressed at full velocity. Not recorded in the practice history (that is for the MIDI
keyboard). Checked by hand on the host (Linux).

The plugin tests (`crates/engine/tests/plugin.rs`) now run one at a time: with Dexed installed
(it sorts before Surge XT, so it became the test plugin) four instances on parallel threads
hung. The app only ever uses plugins from one thread.

## Metronome

`crates/engine/src/metronome.rs`. The clicks are made inside the audio callback, after the
plugin, so every beat lands on its exact sample whatever the block size or UI load (never UI
timers). The beat position is kept as a fractional frame count, so odd tempos do not drift.
Every click is the same. Three sounds, each two decaying sines starting at their peak: Tick
(1500 Hz, bright, 12 ms), Woodblock (820 Hz, hollow knock), Beep (1000 Hz, longer, fades within
about 0.2 s so it fits 300 BPM).

- `Metronome` (shared, atomics): on/off, BPM (20 to 300), volume (0 to 1, linear; default 0.6),
  sound, and `play_sample()` (one click now, metronome on or off, to try a sound).
- Window: in the status bar's right part, a metronome button (a drawn metronome icon, highlighted while it runs) and a BPM field (drag, scroll or
  type). Volume and sound are in the options window; changing either plays a sample click. Space starts and stops it. A Tap button (or T) sets the
  tempo from the average of the last 5 taps; a pause over 3.2 s starts a new count; the beat
  is not moved onto the taps (`TapTempo`). On purpose nothing more: the user wants no beats per bar,
  accents or beat LEDs (an earlier version had them).
- `onezan play --metronome BPM` and `onezan gui --metronome BPM` start it at that tempo.
- Checked: unit tests (beats on their sample for blocks of 64 to 4096 frames, no drift at 70
  BPM, off/on), and recordings of the system output: clicks exactly 500.0 ms apart at 120 BPM
  and 666.7 ms at 90, all the same level. A unit test checks the volume scales the click.
- Tempo, click volume and sound are saved between starts (see "Saved between starts").

## Scale fingering

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

## Chords

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
  line, the dimmed last chord, badges.

## Chord fingering

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

## Built-in piano

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
  put back keeps its old time).

**Volume**: the built-in piano's page (in place of a
plugin's window: `piano_page`) shows a drawn upright piano (`upright_piano`: outlines in the
text's grey, faint fills, no background; both themes), its name and credit, and a volume slider,
(first in Options; moved here by the user's choice), 0 to 200 % of the SoundFont's own level (100 % is how it sounded before), through rustysynth's
master volume, shared with the voice as an atomic and applied from the next block. Saved as
`piano_volume` (left out at 100 %); kept while a plugin plays and applied again when the piano
comes back. Test: `tests/piano.rs` (half volume, half the level; 0 silent; clamped).

**Appearance**: Options has Dark (the default, the user's
choice) or Light, applied at once with `ctx.set_theme` and saved as `"theme": "light"` (left out
for dark). It no longer follows the system's setting. The button styling (`style_buttons`)
covers both themes. The falling-notes area stays dark in both (fixed colours, like piano videos);
the mode button's violet works on both.

## Falling notes

The user's idea: use the space the plugin's editor takes for the "falling notes" way of learning
a piece, as piano videos and games show it, mainly to learn songs from MIDI files. The bar's
mode button (`mode_button`: filled violet, with a drawn icon, right after the pedal light, as the
switch between the app's two sides) reads **Learn Song** (falling-notes icon) in the instrument's view
and **Edit instrument** (sliders icon) in the notes view, "Learn Song ▶ 3/20" while a song runs
behind the instrument's window. It is in the bar's left part, which is cut off first when the
window is narrow. It switches the top between the editor (hidden, not closed: X11 unmap /
`ShowWindow(SW_HIDE)`) and the notes. In the notes view the bar and the chord line move to the
top, so the notes fall straight onto the keys.

- **What falls**: a song opened with "Open song…" (rfd, through the desktop portal on Linux; the
  dialog runs on its own thread) or dropped on the window; else the scale chosen, two octaves
  hands together up and down, a note per beat, with its fingering on each note. "Close" by its name goes
  back to the scale. The song's file, the view and the mode are saved in settings.json
  (`song`, `notes_view`, `wait`); a song that no longer reads is dropped with the reason shown.
- **MIDI files** (`Song::from_midi`, the `midly` crate, MIT/Apache, without its rayon feature):
  every channel but drums; beats from the file's ticks (a quarter note is a beat); tempo changes
  are not followed, and the first tempo becomes the metronome's when a song is opened. Hands: a
  part per track (per channel in a one-track file); track names (left/LH/bass, right/RH/treble/
  melody) decide, else the highest part is the right hand; one part splits at middle C. No
  fingers for songs.
- **Play along** (default): Start (or Space in this view) switches the metronome on, counts in four
  beats, and the notes reach the line on the clicks. The metronome publishes its beat position
  from the audio callback (`Metronome::beat_now`, a sequence lock; the block is heard about one
  block after the callback), so drawing follows the clicks, not a UI timer. A note counts when
  played within 0.15 s of its beat (at most 0.45 beat); late ones turn grey. Score and streak in
  the strip, a summary at the end.
- **Click on or off**: with a song open, in the notes view the bar's metronome button switches the click (lit:
  heard), not the metronome: playing along needs its time either way. Unheard, it keeps the time
  (`Metronome::set_silent`: the beat still counts), only while a run plays along in the notes
  view; elsewhere, and whenever no song is open, the button starts and stops the metronome
  as before (and the click is never silenced). Saved as `notes_silent`.
  Set before the run switches the metronome on, or its first click sounded (found by
  recording). Checked by recording: clicks on every beat with it, silence without. (First a
  "Click" checkbox in the strip; the user wanted the existing button instead.)
- **The scale is not part of the notes view** (the user's decision): it is chosen only with the
  bar's Scale button and shows on the keyboard as everywhere, also while a song falls. The strip
  shows only the song ("Song: name", Close, Open song…). With no song open, the scale chosen falls
  (unmentioned); a scale chosen never replaces an open song. (Two earlier versions had scale and
  song buttons in the strip to switch between them; confusing, two places for the scale.)
- **The song goes on in the instrument's view**: the run is moved on every frame whichever view
  is shown (`App::tick_run`), so you can switch to the plugin's window and change its sound while
  the song keeps playing (and judging the keys, and the app's notes sounding). The bar's button
  then reads "Learn Song ▶ 3/20"; Space and the metronome button act on the run (start/stop, click
  on/off) as in the notes view. Checked in a throwaway build: switched views 2 s into a run, the
  recorded output kept a note on every beat. The window repaints every frame while a run goes
  (as in the notes view), also with the editor shown.
- **Note names** on the falling notes, at the bottom (above the finger for a scale): a scale's
  notes are spelled by degree (`Scale::letter`: F# major's seventh is E#, Db major has Gb), a
  song's by the MIDI file's first key signature (flats or sharps; `chords::letter_of`), without
  one the usual names (Db Eb F# Ab Bb). Written with # and b like the chord line. Tests in
  `waterfall.rs`.
- **The song's key** next to its name ("C major", dimmed), when the file gives one: its first
  key signature (`Song::key`, `Key::name`). Optional in MIDI files: notation programs write it,
  recordings and DAW exports often not (or a default C major that may be wrong); nothing is shown
  then. Not guessed from the notes (possible later). Key changes later in a song are not shown.
  The key is a toggle button: a click selects its scale (`Key::scale`: major, or natural minor),
  shown on the keys as any scale chosen in the bar; it is lit while that scale is selected, and
  a click then clears the scale.
- **Wait for me**: no metronome; the notes stop at the line until the chord there is played.
- **Play the notes** (a checkbox, like "Wait for me"; off by default, saved as `play_notes`; for fun, the point is to hear what you
  play): the app plays the falling notes through the instrument as they reach the line (channel 0,
  the file's velocity; scales at 80), and the keyboard shows them pressed. Sent from the frame
  loop, so within a frame of the beat, not sample-exact. Not judged as the user's playing; stopped
  notes are released. Checked: keys pressed at the line (screenshot) and sound on every beat after the count-in
  (system output recorded with `pw-record -P stream.capture.sink=true`). It sounds only while a
  run goes (Start or Space): as a highlighted button it looked like it would play by itself.
- Checked in a throwaway build on the host (screenshots, notes through Midi Through): song and
  scale views, count-in, hits, misses, wrong-key flash, waiting. Space sent with `wtype` does not
  reach the XWayland window, so keys and buttons need checking by hand (RELEASE.md).

## Saved between starts

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
  keyboard that is not plugged in → the automatic choice (not waited for). A saved audio output
  that is gone → automatic.
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

## Windows: a GUI program

`#![windows_subsystem = "windows"]`: started from Explorer or a shortcut, no console window
opens. `src/console.rs`: started from a terminal, the app attaches to it (`AttachConsole`), so
the report and the command-line commands print there; a command-line command with nowhere to
write (started from Explorer) opens a console of its own; redirected output (`> file`) is left
alone. A GUI program does not make cmd/PowerShell wait: the prompt comes back at once, and the
terminal competes for the keyboard (`play`'s "press Enter"; Ctrl+C works). Checked under Wine:
subsystem 2 (GUI) in the PE header; `list` into the terminal and into a file; `wine start
onezan.exe` opens only the window; `wine start onezan.exe play --demo` opens a console with the
report.

## Windows checks under Wine

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
latency and the editor need a Windows PC (see RELEASE.md).

## Linux release build

`build/linux-release.sh` builds in an Ubuntu 20.04 container (`build/linux.Dockerfile`), with
rootless Podman (preferred: no root, no daemon, no docker group; the user's choice) or Docker.
Ubuntu 20.04 was the user's choice for the widest reach without going ancient; it beats Debian
11 (same glibc) because its package archive stays online during extended support (to 2030).
Result: needs glibc 2.30+, links only libc/libm/libdl/libpthread/libgcc_s and libasound.so.2;
GL, X11/Wayland and JACK are loaded at runtime. Checked: the binary renders and opens the window
on Arch. First build about 2 minutes; crates are cached in `target/container-cargo-home`.

## Windows release zip

`build/windows-release.sh [DIR]` cross-builds with MinGW (no container: the exe links only to
Windows' own DLLs, including the Universal C Runtime, `api-ms-win-crt-*`, part of Windows 10+),
strips it (27 to 20 MB), and zips it with README.md and LICENSE as
`target/windows-release/onezan-VERSION-COMMIT-windows-x64.zip` (about 8 MB); with DIR it copies
the zip there. It fails if the exe would need a MinGW runtime DLL (libgcc, libstdc++,
libwinpthread). Uncommitted changes make the name `COMMIT-dirty`. The release zips have been run
on real Windows by the user.

## Lints

Strict clippy, from https://www.namtao.com/rust/ ("Strict lints"): `[workspace.lints.clippy]` in the
root Cargo.toml (pedantic + nursery denied, plus the panic lints: unwrap/expect/indexing/
arithmetic/`as`/...), each crate opts in with `[lints] workspace = true`, and `clippy.toml` allows
unwrap/expect/panic/indexing in tests. `cargo clippy --all-targets` is clean on Linux.

- VST3 enum constants go through `util::venum` (checked `try_from`): they are `c_uint` on Linux
  and `c_int` on Windows, so `as` would be wrong on one of them. Counts go through `util::int32`.
- Two justified `#[expect]`s: `Module::load` keeps the cache locked on purpose
  (`significant_drop_tightening`), and `unsafe impl Send for Processor` (`non_send_fields_in_send_ty`).
- Also clean on Windows: `cargo clippy --target x86_64-pc-windows-gnu --all-targets`.
- `rustfmt.toml` sets `max_width = 120`; the code is formatted with it.

## Known gaps and shortcuts in the code

- Live MIDI is stamped with `Instant::now()` when midir's callback runs, not with the driver's
  timestamp (midir's is on another clock, and differs per platform). Good to well under 1 ms.
- `restartComponent` and `performEdit` from the plugin are ignored.
- Plugins run in our own process, so a plugin crash takes the app down. One that crashes while
  it is scanned, loaded or opens its editor is skipped at the next start (see "Plugins that
  crash onezan"); a crash while it plays is not caught.
- macOS module loading is not implemented (not a target).

## Reference code (consulted, not dependencies)

All of these use `vst3` 0.3 and are licensed compatibly with GPL-3:

- **transit**, https://github.com/transit-audio/transit (Apache-2.0). `crates/vst3-host/src/ffi/` holds
  `module.rs` (loading, entry points), `instance.rs` (set-up, `process`), `events.rs`,
  `host_context.rs`, `host_objects.rs` (IMessage, IAttributeList, MemoryStream/IBStream),
  `view.rs` and `run_loop.rs` (the editor window and the Linux IRunLoop).
- **suhina**, https://github.com/suhina-daw/suhina-daw (GPL-3.0). `crates/plugin-host` uses
  clack-host and vst3, and runs plugins out of process.
- Maolan (BSD-2), Lemon Studio (GPL-3), tripplyons/daw (MIT).
- Gridline (C++, GPL-3) has its own hosting code on Steinberg's raw headers. Not reusable from Rust.

Not kept in this repo; clone them when needed.
