# onezan

**Practice piano scales with your MIDI keyboard.** onezan shows the fingering for every major
and minor scale on an on-screen keyboard, plays what you play through a real VST3 instrument
(or its own piano), and keeps track of how much you practiced each scale.

For Linux and Windows. Free and open source (GPL-3.0-or-later).

![onezan playing through Surge XT: E natural minor's fingering on the keyboard, and an E minor
chord held, named and fingered](docs/screenshot.png)

## What it does

- **Fingering for every scale.** Major, natural minor, harmonic minor and melodic minor in all
  12 keys, for both hands: numbered badges on the keys show which finger goes where.
- **Clef and key signature** next to every scale in the list, so you see at a glance how it is
  written.
- **Chord names.** Hold a chord and onezan names it (root, type, inversion), with a fingering.
- **Your sound.** Plays your keyboard through any VST3 instrument you have installed, with its
  own editor in the window. Without one, it uses its built-in piano.
- **A metronome**, with a tempo you can tap in.
- **Practice history.** How long you practiced each scale, and when you last played it.
  Star your favorite scales and instruments to keep them at the top.

## It just works

Start it and play. onezan finds your MIDI keyboard, the best audio output and an instrument by
itself, and remembers what you used last time. Plug the keyboard in later, or unplug and
replug it: onezan picks it up. Everything can be changed in the window, but nothing has to be.

## Getting started

1. Download onezan for your system from the Releases page: an AppImage or a plain binary for
   Linux, a zip for Windows. No installation needed.
2. Connect your MIDI keyboard and start onezan.
3. Click **Scale** in the bar above the keyboard and pick a scale. Its fingering appears on the
   keyboard: right hand in blue, left hand in orange.

Useful to know:

- **Space** starts and stops the metronome; **T** taps the tempo.
- You can play the on-screen keyboard with the mouse, and click the **Pedal** light to hold the
  sustain pedal.
- Hover over anything in the bar to see what onezan chose and why.

### Linux: lower latency

onezan prefers JACK when it is there. On PipeWire systems that is the `pipewire-jack` package:
with it, onezan gets short audio blocks without any setup. On Debian, Ubuntu and Mint,
PipeWire's JACK library is not on the library path; start onezan with `pw-jack onezan` (onezan
tells you when this applies).

### Where onezan looks for instruments

VST3 instruments are found in `~/.vst3`, `/usr/lib/vst3` and `/usr/local/lib/vst3` on Linux,
`%CommonProgramFiles%\VST3` on Windows, and in any folders listed in `$VST3_PATH`. A free one to
start with is [Surge XT](https://surge-synthesizer.github.io/).

## Where the fingerings come from

### Scales

onezan shows the standard fingering taught for each scale. It doesn't invent fingerings. Every
scale was compared with at least two independent published charts:
[pianoscales.org](https://pianoscales.org/major.html),
[Hoffman Academy's Melodic Minor Scale Fingering Guide](https://cdn.hoffmanacademy.com/blog/wp-content/uploads/2022/04/Melodic-Minor-Scale-Fingering-Guide.pdf),
[Greg Spero's scale lessons](https://composition.gregspero.com/lessons/a3/),
[musicandhealth.co.uk](https://musicandhealth.co.uk/scales_examples/fingering1.htm), and a
[teaching chart from the University of Miami](https://www.cs.miami.edu/home/burt/music/finger-charts.html)
that shows where the fourth finger falls in every key. The sources agree on almost every note.
Where they differ, it's only about which finger starts or ends a scale in a few keys; onezan
follows the majority. The numbers from these charts are built into onezan's tests, so the
fingerings can't change by accident, and the tests check that every scale can be played up and
down over one to four octaves with the thumb only on white keys.

### Chords

There is no official fingering for chords: exam boards print broken chords and arpeggios, and
teachers differ. So onezan shows a chord fingering only where at least two published sources
give one and none gives another, and shows nothing otherwise. The sources are the Salvation
Army's music manual ("Piano Chords in Inversions", all 24 keys, both hands),
[musicandhealth.co.uk](https://musicandhealth.co.uk/)'s key-by-key broken chords,
[Hoffman Academy's chord inversions](https://www.hoffmanacademy.com/), Piano with Lauren's
chart, and Yamaha's "Basic piano chords for beginners".

That covers major and minor triads in close position and in each inversion, and the same with
the bottom note doubled an octave up (C E G C). For example, a right-hand triad in root position
is 1 3 5 and in first inversion 1 2 5; the left hand plays 5 3 1 in root position and 5 2 1 in
second inversion.
Where the sources disagree (the right hand's second inversion, 1 3 5 or 1 2 4, is the best-known
case), onezan shows no number rather than pick a side. Seventh chords and wider voicings have no
fingering yet. As with the scales, the sources' numbers are built into the tests.

Many good fingerings exist, and teachers and exam boards sometimes prefer others. If your
teacher gives you a different one, follow your teacher.

## Building from source

See [docs/BUILDING.md](docs/BUILDING.md).

## Credits and license

onezan is free software, licensed under the GNU General Public License, version 3 or (at your
option) any later version (GPL-3.0-or-later). See `LICENSE` for the full text.

- **Built-in piano:** [FreePats](https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html)'
  "Upright Piano KW", a Kawai upright recorded by Gonzalo and Roberto in 2017, released under
  the Creative Commons CC0 1.0 public domain dedication. Played with
  [rustysynth](https://github.com/sinshu/rustysynth) (MIT).
- **Music symbols:** [Bravura](https://github.com/steinbergmedia/bravura), © Steinberg Media
  Technologies GmbH, under the SIL Open Font License 1.1 (`assets/fonts/OFL.txt`), embedded
  unchanged.
- **VST** is a registered trademark of Steinberg Media Technologies GmbH.
