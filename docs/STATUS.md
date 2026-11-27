# onezan: status

Last updated: 2026-11-27. Read this first when resuming.

## Where we are

**Branch `polish-0.3`** (since 2026-11-27): 0.2.0 stands as released for a while. The polish work
goes onto this long-lived branch and becomes 0.3.0 only when the app has real polish; master
stays at 0.2.0 until then. Done there so far: plugins that crash onezan are skipped at the next
start, and scanning is cached (ROADMAP.md, Part 1, item 2).

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

How each part works: DESIGN.md. Hand checks still to do: QA.md. Release steps and the full hand
checklist: RELEASE.md. Tests: TESTING.md.
What could come next, and how people find onezan: ROADMAP.md (a proposal, written 2026-11-27).

## Releases

- **0.2.0** (2026-11-26): the state at the scope reset, tagged as is; the hand checklist in
  RELEASE.md was not yet gone through for it.

## Open decisions

- **Which known limits are fine for the release** (the list below), and which must be fixed.

## Rough edges known

To go through one by one: fix, or accept and say so (README or DESIGN.md).

**Not yet checked by hand**: the list is in QA.md.

**Known problems**
- Without OpenGL 2.0 (no graphics driver, a VM) onezan cannot start. It now says so in a message
  box, with the reason and "install the graphics driver" (on `polish-0.3`; on Linux the box needs
  zenity). Still open: whether to run anyway, with a renderer that does not need OpenGL (wgpu:
  DirectX 12 has a software fallback on Windows).
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
- Plugins run in onezan's own process: a plugin crash takes the app down. One that crashes while
  it is scanned, loaded or opens its editor is skipped at the next start (on `polish-0.3`); a
  crash while it plays is not caught.
- Falling notes ignore tempo changes in a MIDI file (the first tempo is used) and show no
  fingers for songs.
