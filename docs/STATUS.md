# onezan: status

Last updated: 2026-11-26. Read this first when resuming.

## Where we are

**Scope reset on 2026-11-26.** What onezan does today is the release. No new features until it is
out: the work now is fine-tuning, fixing rough edges and testing by hand on Linux and Windows,
until there is nothing left that a new user would trip over.

The plans made before the reset (scale exercises with hands separately and contrary motion, a
grand staff with notation, ASIO, CLAP plugins, more falling-notes features) are dropped, not
postponed. What comes after the release is decided from scratch then. The old notes are in git:
`git show 10463f2:docs/STATUS.md`.

## What the release contains

- Plays a MIDI keyboard through any installed VST3 instrument, with its own editor embedded in
  the window, or through the built-in piano (with a volume control).
- Automatic choice of MIDI keyboard, audio output (JACK, else ALSA; WASAPI on Windows),
  instrument and MIDI timing; each can be changed in the window. Keyboards can be plugged in
  and out while it runs.
- An on-screen keyboard (49 keys, widening when needed) showing velocity, the pedal, the chosen
  scale and its fingering for both hands; playable with the mouse.
- Fingering for 48 scales (major, natural, harmonic and melodic minor in all keys), with clef and
  key signature in the scales window; chord names and, where sources agree, chord fingering.
- A metronome with tap tempo.
- Practice history per scale; favorite scales and instruments.
- Falling notes ("Learn Song"): a MIDI file or the chosen scale falls onto the keys, played
  along with the metronome or waiting for each chord.
- Dark or light theme. Everything is remembered between starts, best effort.
- Releases: a Linux tarball and AppImage (glibc 2.30+), a Windows zip; built by GitHub Actions
  from a tag (see BUILDING.md).

How each part works: DESIGN.md. Hand checks and release steps: RELEASE.md. Tests: TESTING.md.

## Open decisions

- **Version number** of the release (Cargo.toml says 0.1.0; 1.0.0 was the earlier plan).
- **Which known limits are fine for the release** (the list below), and which must be fixed.

## Rough edges known

To go through one by one: fix, or accept and say so (README or DESIGN.md).

**Not yet checked by hand** (see the checklist in RELEASE.md)
- On real Windows: the clef column in the scales window, the exe's icon in Explorer and the task
  bar, falling notes, MIDI keyboards replugged while running.
- Falling notes anywhere: the file dialog, dropping a file on the window, Space in the notes view,
  the mode button at narrow window widths (the bar's left part is cut off first).
- Exact vs. immediate MIDI timing, by ear (only checked offline: no system here gives short
  enough blocks).

**Known problems**
- The mod wheel has no audible effect in Surge XT's default patch; unclear whether anything is
  wrong.
- The octave button on a held note (set aside by the user earlier; to look at again).
- Under Wine, after the window grows, a strip at its right edge can stay unpainted until the next
  repaint (seen once). Not seen on real Windows.
- Keyboard focus inside the embedded plugin editor is not handled (typing into a plugin's text
  field may not work).
- Under fractional scaling the window (XWayland) may look soft.
- A plugin editor that keeps its proportions (Surge XT) leaves an empty dark band between it and
  the bar when the window is taller than the editor needs (about 180 px in a square window).

**Noise and naming**
- The JACK client is called `cpal_client_out` (cpal 0.16 fixes the name).
- Without JACK, pipewire-jack prints two warnings to stderr before the fallback; listing ALSA
  outputs prints "Unknown PCM" lines.

**Limits by design** (probably fine for the release; decide)
- Plugins run in onezan's own process: a plugin crash takes the app down. Scanning loads every
  plugin in-process, with no cache.
- Falling notes ignore tempo changes in a MIDI file (the first tempo is used) and show no
  fingers for songs.
