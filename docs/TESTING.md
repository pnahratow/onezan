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

What exists is marked ✓; the rest is the plan.

### vst3-host (the plugin host)

| Contract | Test |
|---|---|
| MIDI bytes parse into events (note-on velocity 0 is a note-off, system messages ignored) | ✓ `midi.rs` |
| Plugin state bytes round-trip and damage is rejected | ✓ `plugin.rs`, `stream.rs` |
| Class ids print the same on every platform | ✓ `util.rs` |
| Editor run loop: handlers and timers run, stop when unregistered | ✓ `run_loop.rs` |
| Scanning: a folder with a broken or non-VST3 bundle gives the others, never an error | new: a temp folder with a fake bundle |

### engine

| Contract | Test |
|---|---|
| **Instrument** (any): a note at frame N sounds from N (within 1 ms); a released note dies away; the sustain pedal holds and releases; events past the block's end are not lost | ✓ for plugin and piano, as two copies (`tests/plugin.rs`, `tests/piano.rs`) → **one shared suite** run on both |
| **Callback**: every queued event reaches the voice, at its frame, also when a callback is split into parts; stereo, mono and multichannel outputs are filled correctly; the metronome is mixed in | new, with a fake voice that records what it got (needs the seam below) |
| Timing: exact maps arrival times onto the block, immediate puts all at 0; auto switches at 11 ms | ✓ `timing.rs` |
| Metronome: beats on their sample for any block size, no drift, on/off, volume | ✓ `metronome.rs` |
| MIDI in: the keyboard is picked over loopbacks; a query matches; replugging releases held notes; switching ports releases held notes | ✓ `midi_in.rs`, `tests/midi_hotplug.rs` (Linux) |
| Audio out: the choice is explained (report) | ✓ partly; the choice of output as a pure function of the outputs found (new) |

**Seam needed:** `Callback` drives `Voice`, an enum. A small trait (`Render`: "process these
events into these buffers") that `Voice` implements lets the callback run with a test voice. No
change to how the app behaves.

### app

| Contract | Test |
|---|---|
| Fingering tables match their sources and are playable | ✓ `fingering.rs` |
| Chord names: every root × chord type × inversion is named back as itself; ties settled by the bass | ✓ examples → **exhaustive round trip** (build each chord, read it, compare) |
| Chord fingering only where sources agree | ✓ `chords.rs` |
| **Instrument choice**: asked for → that one or an error; remembered → that one, else automatic; nothing installed or loadable → built-in piano, marked as a stand-in and not remembered; a built-in piano chosen by hand is remembered | new: move the decision out of `plugins::load` / `session::load_instrument` into a plain function over (asked, remembered, what is installed, what loads), then a table of cases |
| **Keys shown**: per key, velocity (MIDI or mouse), scale tint, scale fingering, a held chord's fingering in place of the scale's, and the outlined style | new: move the per-key decision out of `gui.rs::keyboard` into a plain function, test the combinations |
| Settings: missing and unknown keys are fine, a broken file is moved aside, unchanged settings are not written | ✓ `settings.rs` → plus **a fixture** of today's file |
| Practice history: pauses do not count, notes carry their scale, a damaged file is moved aside | ✓ `progress.rs` → plus **a fixture** of today's database (schema `user_version = 1`) |
| Instrument sound: a damaged, refused or crashed state is moved aside and the default used | new: the file handling in `instrument_state.rs` is testable without a plugin if it takes the save/restore as a function |
| Startup report: the item used is first and says why; each alternative says why not | ✓ partly (`audio_out.rs`) → one test per section on the facts, not the wording |

### Left to hand checks (on purpose)

The window's look and layout, the embedded plugin editor, real audio devices and latency, real
MIDI hardware, and Windows itself (beyond Wine). These change often or need the real thing;
tests would be brittle or impossible. STATUS.md lists what was checked by hand and when.

## Order of work

Each step is small, stands on its own, and changes no behaviour. Steps 1 to 7 were done on
2026-11-25 (below, what each became); the tables above still mark the rest as new.

1. **Shared instrument suite**: merge the plugin and piano checks into one set of contract
   tests, run on the piano (always, when the SoundFont is there) and on a plugin (when installed).
2. **Callback seam and tests**: the `Render` trait, a recording fake voice, tests for event
   placement across split callbacks and for channel layouts.
3. **Instrument choice as a function**, with its table of cases (the newest logic, and the one a
   change most easily breaks: fallback, remembering, errors).
4. **Saved-format fixtures**: a settings.json, a progress database and an instrument state
   written by today's version, in `tests/fixtures/`, loaded by tests.
5. **Keys shown as a function** in the window, with tests for scale and chord fingering together.
6. **Chord naming round trip** over all roots, types and inversions.
7. **One check command**, `build/check.sh`: format, clippy for Linux and Windows, tests on Linux
   and under Wine; what to run before a commit. (No CI yet; this is the same thing by hand.)

What they became:

1. `crates/engine/tests/common/mod.rs`: `instrument_contract!(make)` writes five tests (a note
   starts at its own frame, a released note dies away, the pedal holds, a ten-note chord sounds
   and stops, an event at a block's end is not lost); `tests/piano.rs` and `tests/plugin.rs` use
   it. Checked by breaking the piano's timing on purpose: the frame test failed.
2. `callback::Render` (implemented by `Voice`); eight tests in `callback.rs` with a recording
   fake: order, every source, a flood delayed not lost, exact timing inside the block, a
   callback split in parts (checked by breaking the part offsets on purpose), mono, stereo and
   four channels, the metronome, the block size report.
3. `plugins::choose` over a `Source` (`Installed` for real); five table tests with a fake source.
4. `apps/onezan/fixtures/` (settings, practice history, with a README) and
   `crates/vst3-host/fixtures/` (an instrument sound); one test each reads them.
5. `KeyLooks` in `gui.rs`; four tests (scale alone, chord over it, one hand agreed, no chord).
6. Three tests over all 12 roots × 20 chord types × every inversion. They found a bug: a minor
   triad in first inversion was named as a sixth chord without its fifth (Cm/Eb read as Eb6).
   Only sevenths and ninths may now drop their fifth.
7. `build/check.sh` (`--quick`: Linux only).

## Writing a new test

- Name it as the promise it checks, in words (`a_released_note_dies_away`), as the existing
  tests do.
- Put it next to the module (`#[cfg(test)] mod tests`) when it tests that module's contract;
  in `crates/*/tests/` when it needs a plugin, a MIDI port, files or several modules.
- Use tolerances for audio and time, exact values for data from sources and file formats.
- A test that needs something the machine may lack checks for it first and returns with a note.
