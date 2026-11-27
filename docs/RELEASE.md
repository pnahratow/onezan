# Releasing onezan

A release is checked by hand on the files the release workflow built, on Linux and on real
Windows, from top to bottom. Wine is not Windows: audio, MIDI, latency and the plugin editor need
the real thing. Tests (`build/check.sh`) cover the rest; see TESTING.md.

## Steps

1. Everything for the release is merged on master; `build/check.sh` passes; CI is green.
2. Set the version in `Cargo.toml` (`[workspace.package]`), commit.
3. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`. The release workflow builds the
   Linux tarball, the AppImage and the Windows zip, and makes a **draft** release with them.
4. Download the files from the draft and go through the checklist below with them (not with a
   development build).
5. Write the release notes in the draft, publish it.

A problem found in step 4: fix it on master, delete the draft and the tag, and start again.

## Hand checklist

Mark each line per system with the version or commit it was checked on. Start from a fresh
setup where the line says so: move `~/.config/onezan` and `~/.local/share/onezan` aside
(`%APPDATA%\onezan` on Windows).

### Starting

| Check | Linux | Windows |
|---|---|---|
| Downloads start as they are: AppImage, tarball binary, the exe from the zip (no console window from Explorer) | | |
| Icon: AppImage in the menu/launcher, the window's title and task bar, the exe in Explorer | | |
| Fresh setup: it starts, makes sound with the keyboard plugged in, nothing to set up | | |
| Fresh setup without any VST3 plugin: the built-in piano plays, the top says why | | |
| Linux on PipeWire without JACK: the hover report suggests `pw-jack` (Debian/Ubuntu/Mint) | | — |

### Playing

| Check | Linux | Windows |
|---|---|---|
| Notes, velocity on the keys, sustain pedal, pitch bend | | |
| Fast repeated notes and big chords: nothing hangs or drops | | |
| Unplug and replug the keyboard while playing: no hanging notes, it reconnects | | |
| Start without a keyboard, plug it in later | | |
| On-screen keyboard with the mouse, glissando; the pedal light with the mouse | | |
| MIDI menu: choose a port, back to Automatic | | |

### Instruments

| Check | Linux | Windows |
|---|---|---|
| Instruments window: list, favorites, Rescan, switching between two plugins and the piano | | |
| The plugin's editor: embedded, fits, follows window resizes, mouse works, tooltips | | |
| A changed plugin sound comes back after a restart | | |
| A plugin that crashes (a test bundle that aborts, see DESIGN.md "Plugins that crash onezan"): the next start skips it; the instruments window shows it greyed out or under "Skipped"; clicking it or "Try again" tries it again | | |
| Built-in piano page: drawing, volume slider (heard, remembered) | | |

### Options

| Check | Linux | Windows |
|---|---|---|
| Audio output: choose another, back to Automatic; a failing one falls back and says why | | |
| MIDI timing: auto, exact, immediate | | |
| Click volume and sound (a sample click plays) | | |
| Dark and light theme: every window readable in both | | |

### Scales, chords, metronome

| Check | Linux | Windows |
|---|---|---|
| Scales window: clef and key signature column, order by learning order / name, favorites, No scale | | |
| A scale's fingering on the keyboard; widening when a scale reaches past the keys | | |
| Chords: name, notes, fingering badges; the last chord dimmed | | |
| Metronome: button, Space, BPM field (drag, scroll, type), Tap and T | | |
| Practice time shows up in the scales window after playing a scale | | |

### Falling notes

| Check | Linux | Windows |
|---|---|---|
| Learn Song / Edit instrument switch; the editor comes back intact | | |
| Open song… (file dialog) and dropping a MIDI file on the window | | |
| Play along: count-in, notes reach the line on the clicks, hits, misses, score, summary | | |
| Wait for me: the notes stop until the chord is played | | |
| Click on/off with the metronome button; Play the notes | | |
| The song's key button selects its scale | | |
| A song keeps going in the instrument's view ("Learn Song ▶ 3/20") | | |
| Space in the notes view | | |
| A narrow window: what is cut off, nothing overlaps | | |

### Between starts

| Check | Linux | Windows |
|---|---|---|
| Everything above that is remembered comes back: tempo, click, scale, window size, instrument, keyboard, audio output, timing, theme, piano volume, song, view | | |
| A damaged `settings.json` is moved aside and the app starts | | |
| Settings from the previous release load | | |
