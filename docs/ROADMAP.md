# onezan: roadmap

Written 2026-11-27. A proposal, not a commitment: the scope reset in STATUS.md still holds, so
everything marked "after the release" waits until 0.x is out and is decided again then.

Two parts: what to do about onezan's weaknesses, and how the people it is for find it.

## Who it is for

People who already play or take lessons, own a MIDI keyboard (often a VST piano too) and want
to drill scales with correct fingering and see which ones they neglect. Linux users most of
all: they have almost nothing else that does this.

Who it is not for: someone who wants to be taught piano from zero. Simply Piano and Flowkey do
that; onezan doesn't try to.

## Part 1: weaknesses

### Not doing

Lessons and a curriculum, a song library, microphone input for acoustic pianos, a phone
version. Each would make onezan a weaker copy of the subscription apps. Say so in the README.

### Now (fits the polish phase)

1. **The RELEASE.md hand checklist** on Linux and real Windows. Nothing below matters if a new
   user trips in the first five minutes.
2. **A crashing plugin is survivable.** Before scanning or loading a plugin, write down which
   one; if onezan dies there, skip that plugin on the next start and say why in the window. One
   bad plugin then can't break every start. A scan cache comes with it and speeds up starting.
   *Done on `polish-0.3` (2026-11-27); waiting for a hand check.*
3. **The first run explains itself.** With no keyboard plugged in, onezan points to Scale,
   Learn Song and the on-screen keyboard instead of showing an empty state.
   *Done on `polish-0.3` (2026-11-27): C major on a first start, tips in the line above the
   keyboard, a note in Learn Song without a song, Surge XT suggested without a plugin.*

### After the release: deepen the niche

4. **Scales as exercises.** onezan sees the timing and velocity of every note, so it can score
   how even a scale is; no other app does that. A tempo ladder (clean at 60, then 66, ...),
   hands separately, contrary motion. First in line.
   *Started on `polish-0.3` (2026-11-27), see docs/research/learning-scales.md for the
   teaching behind it. Done: hands (together an octave apart, one hand, contrary motion) and
   1 to 4 octaves; evenness after a run (timing, loudness, hands apart, lean); the tempo
   ladder with the fastest clean tempo in the scales window.* Next, from the research:
   - ~~**Notes per beat (2, 3, 4) at a fixed beat.**~~ Done: past 208 the ladder goes to 2,
     then 4 notes a click, up to about 10.7 notes a second.
   - **Where the unevenness is:** per position in the scale ("every right-hand thumb is early
     and loud"), the thumb crossings above all.
   - **Rhythm and accent variants** (dotted, reverse-dotted, groups of 3 or 4), then even
     again; staccato and dynamics (crescendo up, diminuendo down) checked from MIDI.
   - **Stages as presets** (one octave hands separately, contrary motion, two octaves hands
     together, ...) and a daily rotation of 2 to 4 keys.
5. **Exam scale lists.** Arpeggios, and ready-made sets such as "ABRSM Grade 3" or "RCM Level 5
   scales". A specific, findable audience with a reason to pick onezan.
6. **Falling notes, the cheap fixes that matter most.** Follow tempo changes in the file, loop
   a section, slow a song down. Not the rest of Synthesia's feature list.
7. **Plugins in their own process.** Only if people report crashes that item 2 doesn't cover.

## Part 2: getting found

### Now

1. **The repo says what onezan is.** The README's first line says what is different ("Drill
   every scale with the right fingering, through your own VST instrument"); a repo description
   and topics (`piano`, `midi`, `vst3`, `linux-audio`, `music-education`); a short GIF at the
   top of the README.
2. **Packages.** AUR first (easy, and onezan is developed on Arch). Then Flathub, the biggest
   way Linux users find apps: check early that the sandbox can be allowed to read the VST3
   folders (`~/.vst3`, `/usr/lib/vst3`, `$VST3_PATH`). On Windows: winget or Scoop.

### At the release

3. **Post where this audience already is**, one post written for each place: the LinuxMusicians
   forum, r/linuxaudio, r/pianolearning, r/piano, a KVR Audio product listing, AlternativeTo
   (as an alternative to Synthesia and Simply Piano), the Pianoteq forum, Show HN and r/rust
   (the Rust and egui angle).
4. **A 1 to 2 minute demo video**: plug in, pick a scale, play through Surge XT. Every post
   above links it.

### The longest payoff

5. **A website with one page per scale.** People search "B flat major scale fingering" every
   day. 48 static pages generated from the same tested fingering data: the keyboard picture,
   both hands, the sources, and "practice this with onezan". Accurate, doesn't age, and brings
   exactly the right people. GitHub Pages is enough.
6. **Teachers.** One teacher who recommends onezan brings a studio of students. The fingering
   sources are what earns their trust: give them their own page on the site.

## Open questions

- Do packaging and the website count as release work under the scope reset, or wait until
  after it? (They are not features of the app.)
- Flathub at the release, or AUR first and Flathub later?
