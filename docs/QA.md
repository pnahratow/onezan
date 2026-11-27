# onezan: hand checks still to do

The hand checks owed by work that is done in code but not yet tried by hand, newest first. Tick
one off by moving it to "Done" with the date, the system and the commit. Things found while
checking go into STATUS.md ("Known problems").

This is the to-do list. RELEASE.md's hand checklist is the full pass over a release build; it
still has to be done for 0.3.0, even for checks done here.

## Owed

### Plugins that crash onezan (branch `polish-0.3`, 2026-11-27)

Done in code, tested automatically, and from the command line on Linux and under Wine. Not yet
seen in the window, nor on real Windows.

Setup: `build/test-plugins/make.sh` builds two bundles that crash on purpose (`make.sh windows`
for Windows, with MinGW). Use a data folder of your own, so the real one stays untouched:

```sh
cargo build
build/test-plugins/make.sh
VST3_PATH=target/test-plugins XDG_DATA_HOME=/tmp/onezan-qa XDG_CONFIG_HOME=/tmp/onezan-qa target/debug/onezan
```

On Windows, copy `target/test-plugins-win` over, set `VST3_PATH` to it and move
`%APPDATA%\onezan` aside first.

| Check | Linux | Windows |
|---|---|---|
| First start: onezan crashes (CrashScan, while scanning). Second start: crashes again (CrashLoad is the first instrument). Third start: plays normally | | ✓ VM 00a4bc5 (command line) |
| The bar's Instrument hover: CrashLoad and CrashScan are listed as not used, saying why | | |
| Instruments window: "Crash On Load" is greyed out with "(crashed onezan)", the hover says why; "CrashScan.vst3" is under "Skipped" | | ✓ VM 00a4bc5 (hover not seen) |
| Clicking "Crash On Load": onezan crashes; the next start is normal again | | |
| "Try again" on CrashScan: onezan crashes; the next start is normal again | | |
| A plugin used last time that crashed: the next start plays another one, and the hover says why | | |
| With your real plugins (no test bundles): the second start is noticeably quicker when you have many plugins; Rescan finds a newly installed one | | |
| No crash dialog stays up on Windows after a crash (Windows Error Reporting may show one: note what it says) | — | |

### Found on the Windows VM (2026-11-27)

The Windows 10 VM (`claude@192.168.122.44`, see "The Windows VM" below) has no OpenGL driver;
onezan ran there with Mesa's software OpenGL. Each finding is in STATUS.md ("Known problems").
Check again once fixed:

| Check | Linux | Windows |
|---|---|---|
| Without OpenGL 2.0 (no graphics driver, a VM): onezan says why it cannot start, instead of exiting silently | | ✓ VM (message box, 2026-11-27) |
| On a 1024×768 screen the window fits: the bar's right end and the keyboard's finger numbers are visible (fixed: made to fit at start, and when sized to a plugin's editor) | | ✓ VM |
| The startup report's hints name what the window has (now "Options, Audio output" and "Options, MIDI timing") | | ✓ VM |
| `onezan list` with nothing found: the folders are shown as paths, one per line | | ✓ VM |

### The first run explains itself (branch `polish-0.3`, 2026-11-27)

Start with no settings: move `settings.json` aside (`~/.config/onezan`, `%APPDATA%\onezan` on
Windows). Wording is a first draft: change what reads wrong.

| Check | Linux | Windows |
|---|---|---|
| First start: C major's fingering is on the keys, the bar says "Scale: C major" | | ✓ VM |
| No keyboard: the line above the keys says "No keyboard yet: plug one in, or click the keys below to try it" | | ✓ VM |
| Keyboard plugged in: "Play C major: right hand blue, left hand orange. Another scale: Scale"; gone after about 8 notes | | |
| Then "Space starts the metronome, T taps the tempo"; gone once the metronome runs | | |
| Then "Learn Song: a MIDI file falls onto the keys, to play along"; gone once Learn Song is opened | | |
| Then the old line, "Play a chord to see its name and fingering"; the tips do not come back after a restart | | |
| Learn Song without a song: "or drop a MIDI file here. Until then, the chosen scale falls." after "Open song…" | | ✓ VM |
| No plugin installed: the built-in piano page suggests Surge XT | | ✓ VM |
| "No scale" chosen in the scales window stays "none" after a restart (not C major again) | | |
| The wording: does each tip read well? | | |

### Scale exercises, step 1: hands and octaves (branch `polish-0.3`, 2026-11-27)

In Learn Song with no song open, two choices next to "Play the notes": the hands (together,
right, left, contrary motion) and the octaves (1 to 4; contrary at most 2). The keys show the
fingering of the choice, and that is what falls.

| Check | Linux | Windows |
|---|---|---|
| Hands together: the hands an octave apart (C major: left C3 to C5, right C4 to C6) | | ✓ VM (shown) |
| Contrary motion: both thumbs on the key-note, the hands apart and back; playing along counts both hands | | ✓ VM (shown) |
| Right / left hand alone, 1 to 4 octaves: falls as shown, playable | | |
| 4 octaves on a 49-key keyboard: the on-screen keyboard widens; what can't be played is clear | | |
| In contrary motion the two hands' first falling note is on one key: only one shows (drawn on top of each other). Fine, or mark it? | | |
| The choices are remembered after a restart | | |

### Scale exercises, step 2: how even it was (branch `polish-0.3`, 2026-11-27)

After a run played along with the metronome, the end screen says how even it was, e.g. "28 of
29 on time · timing ±14 ms · loudness ±5 · hands 9 ms apart · a little late (+12 ms)". Timing
comes from when each key arrived over MIDI (to the millisecond), not from the frame.

| Check | Linux | Windows |
|---|---|---|
| A run played as evenly as you can: are the numbers believable (timing ±10 to 30 ms, loudness ±5 to 10)? | | |
| A run played unevenly on purpose (rushing, a loud thumb): the numbers get worse | | |
| "a little early / late": matches how it felt against the click | | |
| "Wait for me": no timing (it has no beat), loudness only | | |

### Scale exercises, step 3: the tempo ladder (branch `polish-0.3`, 2026-11-27)

A scale played along with the metronome (not "Wait for me"), once through: clean (every note
on time, the timing's spread within ±30 ms) moves the metronome up one marking (88 → 92); a
near miss (90 % on time or more) stays; fewer goes one marking down. The end screen says which.
Every such run is kept in the practice history; the scales window's "Clean at" shows the
fastest clean tempo per scale for the exercise chosen in Learn Song.

| Check | Linux | Windows |
|---|---|---|
| A clean run: "Clean at 88: next 92", the metronome now at 92 | | |
| A near miss: "Not clean yet ...: again at 88"; many misses: "Many notes missed: down to 84" | | |
| The scales window: "Clean at" shows it; another exercise (hands, octaves) shows its own | | ✓ VM (the column, empty) |
| ±30 ms: too strict or too lenient? (a guess: no teaching source gives a number) | | |
| Climbing feels right: one marking per clean run, not too slow, not too fast | | |

### Timing calibration and starting with the I chord (branch `polish-0.3`, 2026-11-27)

The click's beat now follows when the audio system says a block is heard (WASAPI and ALSA say;
JACK partly), not one block on. Options → Timing calibration: the metronome clicks at 80, a key
on each click, 16 times after the first two; the median offset is saved and taken off every
note when judging. A scale in Learn Song also starts by playing its I chord (any inversion).

| Check | Linux | Windows |
|---|---|---|
| Calibrate: the count goes up with each click played; the result reads believably (10 to 60 ms after) | | |
| Calibrate unevenly on purpose: "too uneven to measure", nothing saved | | |
| After calibrating, a scale played well reads "on time" with no early/late lean | | |
| With Bluetooth headphones: the offset is much larger (100 ms or more), and judging still works after calibrating | | |
| The I chord (e.g. C E G, or E G C) starts the run with the count-in; it is not judged; during a run it does nothing | | |
| The Options window: "The audio system reports N ms" under JACK, ALSA and WASAPI | | ✓ VM (WASAPI 10 ms) |

### Results card and count-in (branch `polish-0.3`, 2026-11-27)

The end of a run is a card: stars as the ladder rates it (3 clean, 2 near miss, 1 most notes on
time), the notes on time and the ladder's verdict, and a gauge per measure (timing evenness,
early/late, loudness evenness, hands together) with green / yellow / red zones. A run started
on a click (a chord played in time) counts that click as "one": four clicks of count-in either
way. Not yet seen drawn: the VM has no keyboard, and mouse clicks there were not timed well
enough to finish a run.

| Check | Linux | Windows |
|---|---|---|
| The card: readable, fits the window, stars and gauges match how it went | | |
| Count-in: four clicks with Space and with the I chord, metronome on or off before | | |

### From before the branch (listed in STATUS.md until 2026-11-27)

"earlier": STATUS.md listed these for real Windows only.

| Check | Linux | Windows |
|---|---|---|
| The clef column in the scales window | earlier | ✓ VM 00a4bc5 |
| The exe's icon in Explorer and the task bar | — | task bar and title bar ✓ VM 00a4bc5; Explorer open |
| Falling notes (playing a song along, waiting) | earlier | |
| A MIDI keyboard unplugged and plugged back in while running | earlier | |
| Falling notes: the file dialog, dropping a file on the window, Space in the notes view | | |
| Falling notes: the mode button at narrow window widths (the bar's left part is cut off first; the button shrinks to its icon) | | ✓ VM, about 1000 points wide |
| Exact vs. immediate MIDI timing, by ear (needs short audio blocks: JACK or a low-latency PipeWire setup) | | |

## The Windows VM

A Windows 10 22H2 VM (German) on the host, reachable as `claude@192.168.122.44` with the SSH key
of this machine; VirtIO guest tools installed. It has no OpenGL driver: copy Mesa's
`opengl32.dll` and `libgallium_wgl.dll` (mesa-dist-win, x64) next to `onezan.exe` and set
`GALLIUM_DRIVER=llvmpipe`. Command-line checks run over SSH. The window only shows in the
logged-in session: the scheduled task `onezan-qa` runs `C:\Users\claude\qa\run.ps1` there,
which starts onezan, clicks and takes screenshots as `qa\steps.txt` says (`build/windows-vm/`
has the scripts). Audio there is the VM's emulated device: not for latency or timing by ear.

## Done

(nothing yet)
