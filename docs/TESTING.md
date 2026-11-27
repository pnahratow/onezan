# Testing plan

Goal: change one part of onezan without keeping the whole app in mind, and find out at once when
a change breaks a promise another part relies on. Not the goal: pinning every detail, so that
each improvement means rewriting tests.

## Principles

1. **Test contracts, not insides.** A test describes what a module promises the modules that
   use it ("a note sounds from its own sample", "a damaged settings file never stops the start",
   "a fingering is shown only where two sources agree"). Private helpers, field layouts and the
   order of calls are free to change.
2. **Assert meaning, not form.** Check what the user or the caller depends on: which instrument
   was chosen and whether it is remembered, not the report's wording; that a sound starts within
   a millisecond of its frame, not its exact samples; that the chosen port is the keyboard, not
   the list's order. Exact numbers only where they are the contract (fingerings from sources,
   frames for sample-accurate timing, file formats).
3. **Pure core, thin shell.** Where a decision is tangled with devices, files or the window,
   move the decision into a plain function that takes values and returns values, and test that.
   The shell (open the device, draw the widget) stays small and is checked by hand.
4. **One suite per interface, run against every implementation.** When two things fill the same
   role (a VST3 plugin and the built-in piano; JACK and ALSA one day), one shared set of contract
   tests runs against each, so a new implementation is checked by the same promises.
5. **Saved data is a contract with the past.** Files written by an older onezan (settings,
   practice history, instrument sounds) must keep loading. Keep a sample of each format as a test
   fixture; a change that cannot read it is a breaking change, made on purpose with a migration.
6. **Tests that need the machine skip, never fail, without it.** A real plugin, the SoundFont, a
   MIDI port: when missing, the test says so and passes (as now). Everything else runs anywhere,
   including as Windows binaries under Wine.

## The contracts, module by module

What exists is marked ✓; the rest is open, to do when that part changes next.

### vst3-host (the plugin host)

| Contract | Test |
|---|---|
| MIDI bytes parse into events (note-on velocity 0 is a note-off, system messages ignored) | ✓ `midi.rs` |
| Plugin state bytes round-trip and damage is rejected | ✓ `plugin.rs`, `stream.rs` |
| Class ids print the same on every platform | ✓ `util.rs` |
| Editor run loop: handlers and timers run, stop when unregistered | ✓ `run_loop.rs` |
| Plugin state from an older onezan loads | ✓ `stream.rs` (fixture in `crates/vst3-host/fixtures/`) |
| Scanning: a folder with a broken or non-VST3 bundle gives the others, never an error | open: a temp folder with a fake bundle (by hand: `build/test-plugins/`, see QA.md) |

### engine

| Contract | Test |
|---|---|
| **Instrument** (any): a note at frame N sounds from N (within 1 ms); a released note dies away; the sustain pedal holds and releases; a ten-note chord sounds and stops; an event at a block's end is not lost | ✓ one shared suite (`tests/common/mod.rs`, `instrument_contract!`) run on the piano (`tests/piano.rs`) and a plugin (`tests/plugin.rs`) |
| **Callback**: every queued event reaches the voice, at its frame, also when a callback is split into parts; stereo, mono and multichannel outputs are filled correctly; the metronome is mixed in | ✓ `callback.rs`, with a recording fake behind `callback::Render` |
| Timing: exact maps arrival times onto the block, immediate puts all at 0; auto switches at 11 ms | ✓ `timing.rs` |
| Metronome: beats on their sample for any block size, no drift, on/off, volume; the beat position it publishes; silent clicks keep the time | ✓ `metronome.rs` |
| MIDI in: the keyboard is picked over loopbacks; a query matches; replugging releases held notes; switching ports releases held notes | ✓ `midi_in.rs`, `tests/midi_hotplug.rs` (Linux) |
| Audio out: the choice is explained (report) | ✓ partly; open: the choice of output as a pure function of the outputs found |

### app

| Contract | Test |
|---|---|
| Fingering tables match their sources and are playable | ✓ `fingering.rs` |
| Chord names: every root × chord type × inversion is named back as itself; ties settled by the bass | ✓ `chords.rs`: examples and a round trip over 12 roots × 20 types × every inversion |
| Chord fingering only where sources agree | ✓ `chords.rs` |
| **Instrument choice**: asked for → that one or an error; remembered → that one, else automatic; nothing installed or loadable → built-in piano, marked as a stand-in and not remembered; a built-in piano chosen by hand is remembered | ✓ `plugins.rs`: `plugins::choose` over a fake `Source`, a table of cases |
| **Keys shown**: per key, velocity (MIDI or mouse), scale tint, scale fingering, a held chord's fingering in place of the scale's, and the outlined style | ✓ `gui.rs`: `KeyLooks` |
| Falling notes: a scale or a MIDI file becomes a song (beats, hands); note names spelled by scale degree or key signature; playing along counts notes on time and misses late ones; waiting stops at each chord | ✓ `waterfall.rs` |
| Settings: missing and unknown keys are fine, a broken file is moved aside, unchanged settings are not written; the 0.1 file loads | ✓ `settings.rs`, fixture in `apps/onezan/fixtures/` |
| Practice history: pauses do not count, notes carry their scale, a damaged file is moved aside; the 0.1 database loads | ✓ `progress.rs`, fixture in `apps/onezan/fixtures/` |
| **Plugins that crash onezan**: unchanged bundles come from the cache, updated ones are scanned again; one that failed to load is tried again; the note names the bundle while it is scanned; one that crashed the scan is skipped until updated; one that crashed while loading is listed as such until forgiven; a damaged or older file means scanning again (keeping crashes) | ✓ `known_plugins.rs`, over fake bundles |
| Instrument choice with crashes: the automatic pick passes over a plugin that crashed; the one used last time is skipped if it crashed; asked for by name, it is tried again | ✓ `plugins.rs` |
| Instrument sound: a damaged, refused or crashed state is moved aside and the default used | open: the file handling in `instrument_state.rs` is testable without a plugin if it takes the save/restore as a function |
| Startup report: the item used is first and says why; each alternative says why not | ✓ partly (`audio_out.rs`); open: one test per section on the facts, not the wording |

### Left to hand checks (on purpose)

The window's look and layout, the embedded plugin editor, real audio devices and latency, real
MIDI hardware, and Windows itself (beyond Wine). These change often or need the real thing;
tests would be brittle or impossible. RELEASE.md has the checklist, done on every release.

## Running them

`build/check.sh` before a commit: formatting, clippy and the tests for Linux and, under Wine,
Windows (`--quick`: Linux only; CI runs that). Tests that need a plugin, the SoundFont or a
virtual MIDI port skip without them.

## Writing a new test

- Name it as the promise it checks, in words (`a_released_note_dies_away`), as the existing
  tests do.
- Put it next to the module (`#[cfg(test)] mod tests`) when it tests that module's contract;
  in `crates/*/tests/` when it needs a plugin, a MIDI port, files or several modules.
- Use tolerances for audio and time, exact values for data from sources and file formats.
- A test that needs something the machine may lack checks for it first and returns with a note.
