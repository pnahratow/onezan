//! Falling notes: a song or a scale comes down towards the keyboard, each note reaching the
//! keys when it is to be played, the way piano videos and learning games show music.
//!
//! - [`Song`]: the notes, in beats, with the hand that plays each and its finger if known. Made
//!   from a scale ([`Song::scale`]) or read from a Standard MIDI File ([`Song::from_midi`]).
//! - [`Run`]: one play through a song. It keeps the clock and judges the keys pressed: in time
//!   with the metronome (a note counts when played within a short window of its beat), or
//!   waiting (the notes stop at the keys until they are played).
//! - [`draw`]: the falling notes over the keyboard's key columns.

use crate::fingering::{Exercise, Hand, Kind, Scale};
use crate::keyboard::{LEFT_HAND, RIGHT_HAND, is_black};
use egui::{Align2, Color32, FontId, Rect, Stroke, StrokeKind, pos2};
use std::collections::BTreeMap;

/// Beats counted in before the first note.
pub const COUNT_IN: f64 = 4.0;

/// How many seconds of music the area shows above the keys.
const SECONDS_SHOWN: f64 = 3.0;

/// A note counts as on time within this much of its beat (or less at fast tempos).
const ON_TIME_SECS: f64 = 0.15;

/// A note of a song.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub key: u8,
    pub hand: Hand,
    pub finger: Option<u8>,
    /// When it starts, in beats from the song's start.
    pub beat: f64,
    /// How long it is held, in beats.
    pub len: f64,
    /// How hard it is struck (1 to 127), when the app plays it.
    pub velocity: u8,
    /// The letter it is written with (0 = C), for its name: [`Note::name`].
    pub letter: u8,
}

impl Note {
    /// Its name as written: "F#", "Bb", "E#".
    pub fn name(&self) -> String {
        crate::chords::spell(self.letter, self.key % 12)
    }
}

/// What falls: notes sorted by their beat.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub name: String,
    pub notes: Vec<Note>,
    /// The tempo the song was written at, if it says (a MIDI file does).
    pub bpm: Option<u32>,
    /// Its key, if the file says (notation programs write it; recordings often do not).
    pub key: Option<Key>,
}

/// A key as a MIDI file gives it: its signature's sharps (positive) or flats (negative), and
/// major or minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub sharps: i8,
    pub minor: bool,
}

impl Key {
    /// Its place round the circle of fifths, 0 (seven flats) to 14 (seven sharps).
    fn fifths(self) -> usize {
        usize::try_from(self.sharps.clamp(-7, 7).saturating_add(7)).unwrap_or(7)
    }

    /// The scale of this key: major, or natural minor for a minor key.
    pub fn scale(self) -> Scale {
        // The tonic's pitch class round the circle of fifths, from seven flats to seven sharps.
        const MAJOR: [u8; 15] = [11, 6, 1, 8, 3, 10, 5, 0, 7, 2, 9, 4, 11, 6, 1];
        const MINOR: [u8; 15] = [8, 3, 10, 5, 0, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];
        let i = self.fifths();
        let (roots, kind) = if self.minor {
            (MINOR, Kind::NaturalMinor)
        } else {
            (MAJOR, Kind::Major)
        };
        Scale {
            root: roots.get(i).copied().unwrap_or(0),
            kind,
        }
    }

    /// "Bb major", "F# minor".
    pub fn name(self) -> String {
        // Round the circle of fifths from seven flats to seven sharps.
        const MAJOR: [&str; 15] = [
            "Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#",
        ];
        const MINOR: [&str; 15] = [
            "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#",
        ];
        let i = self.fifths();
        let (names, kind) = if self.minor { (MINOR, "minor") } else { (MAJOR, "major") };
        format!("{} {kind}", names.get(i).copied().unwrap_or("C"))
    }
}

impl Song {
    /// The scale played as `exercise` (see [`Exercise::lines`]), a note per beat, with its
    /// fingering, as the keyboard shows it.
    pub fn scale(s: Scale, exercise: Exercise) -> Self {
        let mut notes = Vec::new();
        for (hand, line) in exercise.lines(s) {
            let last = line.len().saturating_sub(1);
            for (i, (key, finger)) in line.into_iter().enumerate() {
                notes.push(Note {
                    key,
                    hand,
                    finger: Some(finger),
                    velocity: 80,
                    letter: s.letter(key),
                    beat: f64::from(u32::try_from(i).unwrap_or(u32::MAX)),
                    len: if i == last { 2.0 } else { 1.0 },
                });
            }
        }
        notes.sort_by(|a, b| a.beat.total_cmp(&b.beat).then(a.key.cmp(&b.key)));
        Self {
            name: exercise_name(s, exercise),
            notes,
            bpm: None,
            key: None,
        }
    }

    /// Reads a Standard MIDI File. Every channel but the drums (10) counts. Hands: a file with
    /// a track per hand (as notation programs write piano music) gets them from the track names
    /// ("left", "LH", "bass", ...) or else from the pitch, the higher track the right hand; with
    /// one part, notes from middle C up are the right hand's. Tempo changes are not followed:
    /// the notes stay on their beats, and the first tempo is the song's.
    ///
    /// # Errors
    ///
    /// When the file is not a MIDI file, or has no notes.
    pub fn from_midi(bytes: &[u8], name: &str) -> Result<Self, String> {
        use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
        let smf = Smf::parse(bytes).map_err(|e| format!("not a MIDI file: {e}"))?;
        // Beats per tick (a quarter note is a beat). Timecode files: at 120 BPM, two beats a second.
        let per_tick = match smf.header.timing {
            Timing::Metrical(ppq) => 1.0 / f64::from(ppq.as_int().max(1)),
            Timing::Timecode(fps, sub) => 2.0 / (f64::from(fps.as_f32()) * f64::from(sub.max(1))),
        };
        // The first tempo, in microseconds a beat.
        let mut tempo: Option<u32> = None;
        // The first key signature.
        let mut key: Option<Key> = None;
        // Per part: (track or channel, notes); a part is a track, or a channel in a one-track file.
        let mut parts: BTreeMap<usize, Part> = BTreeMap::new();
        let one_track = smf.tracks.len() == 1;
        for (t, track) in smf.tracks.iter().enumerate() {
            // u32 ticks: 4e9 is far beyond any song.
            let mut tick: u32 = 0;
            let mut name_of_track = String::new();
            // Sounding notes: (channel, the note so far).
            let mut open: Vec<(u8, Played)> = Vec::new();
            for ev in track {
                tick = tick.saturating_add(ev.delta.as_int());
                let beat = f64::from(tick) * per_tick;
                match ev.kind {
                    TrackEventKind::Meta(MetaMessage::Tempo(us)) => {
                        tempo.get_or_insert_with(|| us.as_int().max(1));
                    }
                    TrackEventKind::Meta(MetaMessage::KeySignature(sharps, minor)) => {
                        key.get_or_insert(Key { sharps, minor });
                    }
                    TrackEventKind::Meta(MetaMessage::TrackName(n)) => {
                        name_of_track = String::from_utf8_lossy(n).into_owned();
                    }
                    TrackEventKind::Midi { channel, message } => {
                        let ch = channel.as_int();
                        if ch == 9 {
                            continue;
                        }
                        let (key, vel) = match message {
                            MidiMessage::NoteOn { key, vel } => (key.as_int(), vel.as_int()),
                            MidiMessage::NoteOff { key, .. } => (key.as_int(), 0),
                            _ => continue,
                        };
                        if let Some(i) = open.iter().position(|(c, n)| *c == ch && n.key == key) {
                            // A note on for a sounding key ends it first.
                            let (_, note) = open.swap_remove(i);
                            let id = if one_track { usize::from(ch) } else { t };
                            let p = parts.entry(id).or_default();
                            if p.name.is_empty() {
                                p.name.clone_from(&name_of_track);
                            }
                            p.notes.push(Played {
                                len: beat - note.beat,
                                ..note
                            });
                        }
                        if vel > 0 {
                            let note = Played {
                                key,
                                beat,
                                len: 0.0,
                                velocity: vel,
                            };
                            open.push((ch, note));
                        }
                    }
                    _ => {}
                }
            }
        }
        let parts: Vec<Part> = parts.into_values().filter(|p| !p.notes.is_empty()).collect();
        if parts.is_empty() {
            return Err("the file has no notes".to_owned());
        }
        let hands = assign_hands(&parts);
        let signature = key.map(|k| k.sharps);
        let first = parts
            .iter()
            .flat_map(|p| p.notes.iter().map(|n| n.beat))
            .fold(f64::INFINITY, f64::min);
        // Start on the beat the first note is in, keeping the notes' place in the beat.
        let shift = first.floor();
        let mut notes: Vec<Note> = parts
            .iter()
            .zip(hands)
            .flat_map(|(p, hand)| {
                p.notes.iter().map(move |n| Note {
                    key: n.key,
                    hand: hand.unwrap_or(if n.key >= 60 { Hand::Right } else { Hand::Left }),
                    finger: None,
                    velocity: n.velocity,
                    letter: crate::chords::letter_of(n.key % 12, signature),
                    beat: n.beat - shift,
                    len: n.len.max(0.05),
                })
            })
            .collect();
        notes.sort_by(|a, b| a.beat.total_cmp(&b.beat).then(a.key.cmp(&b.key)));
        Ok(Self {
            name: name.to_owned(),
            notes,
            key,
            bpm: tempo.and_then(bpm_of),
        })
    }

    /// The notes that start, and those that end, after `from` and up to `to` (beats): what to
    /// play as the song moves on. Each as (key, velocity).
    pub fn between(&self, from: f64, to: f64) -> (Vec<(u8, u8)>, Vec<u8>) {
        let within = |b: f64| from < b && b <= to;
        let starts = self
            .notes
            .iter()
            .filter(|n| within(n.beat))
            .map(|n| (n.key, n.velocity))
            .collect();
        let ends = self
            .notes
            .iter()
            .filter(|n| within(n.beat + n.len))
            .map(|n| n.key)
            .collect();
        (starts, ends)
    }

    /// When the last note ends, in beats.
    pub fn end(&self) -> f64 {
        self.notes.iter().map(|n| n.beat + n.len).fold(0.0, f64::max)
    }
}

/// Beats a minute from a MIDI tempo (microseconds a beat), rounded, 1 to 1000.
fn bpm_of(us: u32) -> Option<u32> {
    60_000_000u32
        .checked_add(us.checked_div(2)?)?
        .checked_div(us)
        .map(|b| b.clamp(1, 1000))
}

/// A note as read from a MIDI file, in beats from the file's start.
#[derive(Clone, Copy)]
struct Played {
    key: u8,
    beat: f64,
    len: f64,
    velocity: u8,
}

/// "C major", or with the exercise when it is not the usual one: "C major, contrary motion,
/// 2 octaves".
pub fn exercise_name(s: Scale, exercise: Exercise) -> String {
    if exercise == Exercise::default() {
        return s.name();
    }
    let n = exercise.octaves();
    let octaves = if n == 1 {
        "1 octave".to_owned()
    } else {
        format!("{n} octaves")
    };
    format!("{}, {}, {octaves}", s.name(), exercise.hands.name())
}

/// The notes of one track (or channel) of a MIDI file.
#[derive(Default)]
struct Part {
    name: String,
    notes: Vec<Played>,
}

/// Which hand plays each part: by its name, else the highest part the right hand and the
/// others the left; `None` (split at middle C) when there is only one part.
fn assign_hands(parts: &[Part]) -> Vec<Option<Hand>> {
    let named = |p: &Part| {
        let n = p.name.to_lowercase();
        let has = |w: &[&str]| {
            w.iter()
                .any(|w| n.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w))
        };
        if has(&["left", "lh", "bass", "links", "l"]) {
            Some(Hand::Left)
        } else if has(&["right", "rh", "treble", "melody", "rechts", "r"]) {
            Some(Hand::Right)
        } else {
            None
        }
    };
    if parts.len() == 1 {
        return parts.iter().map(named).collect();
    }
    let mean = |p: &Part| {
        let sum: f64 = p.notes.iter().map(|n| f64::from(n.key)).sum();
        sum / f64::from(u32::try_from(p.notes.len().max(1)).unwrap_or(u32::MAX))
    };
    let highest = parts
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| mean(a).total_cmp(&mean(b)))
        .map(|(i, _)| i);
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| named(p).or_else(|| Some(if Some(i) == highest { Hand::Right } else { Hand::Left })))
        .collect()
}

/// How a note went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Coming,
    Hit,
    Missed,
}

/// One play through a song.
pub struct Run {
    pub song: Song,
    /// When the song's last note ends ([`Song::end`]), worked out once.
    end: f64,
    status: Vec<Status>,
    /// The notes wait at the keys until played; otherwise they keep the metronome's time.
    pub wait: bool,
    /// The song's beat now (negative while counting in).
    beat: f64,
    /// Play along: the metronome beat at the song's beat 0, once the metronome has clicked.
    origin: Option<f64>,
    pub streak: u32,
    pub best_streak: u32,
    /// Keys played right and wrong, with when (seconds, the window's clock), for a flash.
    pub flashes: Vec<(u8, bool, f64)>,
    /// The notes played, with how far from their beat and how hard: for [`Run::evenness`].
    pub hits: Vec<Hit>,
    /// Once played through and judged: what the tempo ladder says (empty: nothing).
    pub verdict: Option<String>,
}

/// A note played: by which hand, how far from its beat (milliseconds, late positive; `None`
/// when waiting, which has no beat to keep), and how hard (`None`: the mouse).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub note: usize,
    pub hand: Hand,
    pub offset_ms: Option<f64>,
    pub velocity: Option<u8>,
}

/// How even a run was: what a teacher listens for in a scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evenness {
    pub played: usize,
    pub notes: usize,
    /// How far the notes are from their beats, spread around their average: the timing's
    /// standard deviation, in milliseconds (two notes or more played in time).
    pub spread_ms: Option<f64>,
    /// Their average offset: late positive, early negative, in milliseconds.
    pub lean_ms: Option<f64>,
    /// The loudness's standard deviation, in MIDI velocity (1 to 127).
    pub loudness: Option<f64>,
    /// How far apart the hands strike the notes they play together, on average, in
    /// milliseconds.
    pub hands_apart_ms: Option<f64>,
}

/// The mean and standard deviation of `values`, with two or more.
fn mean_and_spread(values: &[f64]) -> Option<(f64, f64)> {
    let n = f64::from(u32::try_from(values.len()).ok()?);
    if n < 2.0 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    Some((mean, var.sqrt()))
}

/// How a measure of a run rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Good,
    Fair,
    Poor,
}

/// One measure of a run, for a gauge: its value on a scale, how it rates, and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Gauge {
    pub label: &'static str,
    pub value: f64,
    /// The scale: 0 to `max`, or `-max` to `max` when `centred` (early to late).
    pub max: f64,
    pub centred: bool,
    /// Up to here (from 0, either way when centred) good; up to `fair` fair; beyond poor.
    pub good: f64,
    pub fair: f64,
    pub text: String,
}

impl Gauge {
    pub fn grade(&self) -> Grade {
        let v = self.value.abs();
        if v <= self.good {
            Grade::Good
        } else if v <= self.fair {
            Grade::Fair
        } else {
            Grade::Poor
        }
    }
}

/// An offset leaning less than this either way is not mentioned.
const LEAN_MS: f64 = 10.0;

impl Evenness {
    /// Its measures as gauges, those it knows: timing evenness, early or late, loudness
    /// evenness, the hands together. The timing's fair edge is the tempo ladder's clean limit.
    pub fn gauges(&self) -> Vec<Gauge> {
        let mut out = Vec::new();
        if let Some(s) = self.spread_ms {
            out.push(Gauge {
                label: "Timing evenness",
                value: s,
                max: 60.0,
                centred: false,
                good: 20.0,
                fair: crate::ladder::CLEAN_SPREAD_MS,
                text: format!("±{s:.0} ms"),
            });
        }
        if let Some(l) = self.lean_ms {
            let text = if l.abs() < LEAN_MS {
                "on the beat".to_owned()
            } else if l > 0.0 {
                format!("{l:.0} ms late")
            } else {
                format!("{:.0} ms early", -l)
            };
            out.push(Gauge {
                label: "Early · late",
                value: l,
                max: 60.0,
                centred: true,
                good: LEAN_MS,
                fair: 25.0,
                text,
            });
        }
        if let Some(v) = self.loudness {
            out.push(Gauge {
                label: "Loudness evenness",
                value: v,
                max: 20.0,
                centred: false,
                good: 6.0,
                fair: 10.0,
                text: format!("±{v:.0}"),
            });
        }
        if let Some(h) = self.hands_apart_ms {
            out.push(Gauge {
                label: "Hands together",
                value: h,
                max: 60.0,
                centred: false,
                good: 15.0,
                fair: 30.0,
                text: format!("{h:.0} ms apart"),
            });
        }
        out
    }
}

impl Run {
    pub fn new(song: Song, wait: bool) -> Self {
        Self {
            status: vec![Status::Coming; song.notes.len()],
            end: song.end(),
            song,
            wait,
            beat: -COUNT_IN,
            origin: None,
            streak: 0,
            best_streak: 0,
            flashes: Vec::new(),
            hits: Vec::new(),
            verdict: None,
        }
    }

    pub const fn beat(&self) -> f64 {
        self.beat
    }

    pub fn status(&self, i: usize) -> Status {
        self.status.get(i).copied().unwrap_or(Status::Coming)
    }

    /// Notes played on time, and all notes.
    pub fn score(&self) -> (usize, usize) {
        (
            self.status.iter().filter(|&&s| s == Status::Hit).count(),
            self.status.len(),
        )
    }

    /// How even the notes played so far were.
    pub fn evenness(&self) -> Evenness {
        let offsets: Vec<f64> = self.hits.iter().filter_map(|h| h.offset_ms).collect();
        let velocities: Vec<f64> = self.hits.iter().filter_map(|h| h.velocity).map(f64::from).collect();
        let timing = mean_and_spread(&offsets);
        // The hands' offsets on the beats both play.
        let mut by_beat: BTreeMap<i64, (Option<f64>, Option<f64>)> = BTreeMap::new();
        for h in &self.hits {
            let (Some(o), Some(n)) = (h.offset_ms, self.song.notes.get(h.note)) else {
                continue;
            };
            #[expect(clippy::cast_possible_truncation, reason = "beats of a song, small")]
            #[expect(clippy::as_conversions, reason = "no as-free f64 to i64 conversion")]
            let slot = by_beat.entry((n.beat * 1000.0).round() as i64).or_default();
            match h.hand {
                Hand::Left => slot.0 = Some(o),
                Hand::Right => slot.1 = Some(o),
            }
        }
        let apart: Vec<f64> = by_beat.values().filter_map(|&(l, r)| Some((l? - r?).abs())).collect();
        let (played, notes) = self.score();
        Evenness {
            played,
            notes,
            spread_ms: timing.map(|t| t.1),
            lean_ms: timing.map(|t| t.0),
            loudness: mean_and_spread(&velocities).map(|v| v.1),
            hands_apart_ms: (!apart.is_empty())
                .then(|| apart.iter().sum::<f64>() / f64::from(u32::try_from(apart.len()).unwrap_or(u32::MAX))),
        }
    }

    /// Played through: a beat after the last note ends.
    pub fn finished(&self) -> bool {
        self.beat > self.end + 1.0
    }

    /// Moves the clock on. Play along: to the metronome's beat (`None`: not clicking yet, the
    /// song waits at its count-in). Waiting: by `beats`, but not past a note not yet played.
    pub fn advance(&mut self, metronome: Option<f64>, beats: f64, bpm: f64) {
        if self.wait {
            let next = self.next_step();
            let to = self.beat + beats.max(0.0);
            self.beat = next.map_or(to, |n| to.min(n));
        } else if let Some(m) = metronome {
            // The song starts COUNT_IN clicks on: from the next click, or from this one when
            // started on it (a chord played on a click counts it as "one").
            let origin = *self.origin.get_or_insert_with(|| first_count(m) + COUNT_IN);
            self.beat = m - origin;
            let late = window(bpm);
            for (i, n) in self.song.notes.iter().enumerate() {
                if n.beat >= self.beat - late {
                    break;
                }
                if let Some(s) = self.status.get_mut(i)
                    && *s == Status::Coming
                {
                    *s = Status::Missed;
                    self.streak = 0;
                }
            }
        }
    }

    /// The beat of the first note not yet played (waiting).
    fn next_step(&self) -> Option<f64> {
        self.song
            .notes
            .iter()
            .zip(&self.status)
            .find(|(_, s)| **s == Status::Coming)
            .map(|(n, _)| n.beat)
    }

    /// `key` was struck `ago` beats before now (MIDI arrives between frames), as hard as
    /// `velocity` (`None`: the mouse); `time`: the window's clock, for the flash. Returns
    /// whether it played a note.
    pub fn press(&mut self, key: u8, velocity: Option<u8>, ago: f64, bpm: f64, time: f64) -> bool {
        let beat = if self.wait { self.beat } else { self.beat - ago.max(0.0) };
        let found = if self.wait {
            // A note of the next chord (the notes starting together), from a beat before it.
            self.next_step().filter(|&step| beat >= step - 1.0).and_then(|step| {
                self.song
                    .notes
                    .iter()
                    .zip(&self.status)
                    .position(|(n, s)| *s == Status::Coming && n.key == key && (n.beat - step).abs() < 0.05)
            })
        } else {
            let w = window(bpm);
            self.song
                .notes
                .iter()
                .zip(&self.status)
                .enumerate()
                .filter(|(_, (n, s))| **s == Status::Coming && n.key == key && (n.beat - beat).abs() <= w)
                .min_by(|(_, (a, _)), (_, (b, _))| (a.beat - beat).abs().total_cmp(&(b.beat - beat).abs()))
                .map(|(i, _)| i)
        };
        let hit = found.and_then(|i| self.status.get_mut(i)).is_some_and(|s| {
            *s = Status::Hit;
            true
        });
        if let Some(i) = found.filter(|_| hit)
            && let Some(n) = self.song.notes.get(i)
        {
            let offset_ms = (!self.wait && bpm > 0.0).then(|| (beat - n.beat) * 60_000.0 / bpm);
            self.hits.push(Hit {
                note: i,
                hand: n.hand,
                offset_ms,
                velocity,
            });
        }
        if hit {
            self.streak = self.streak.saturating_add(1);
            self.best_streak = self.best_streak.max(self.streak);
        } else if beat > -1.0 {
            // Before the count-in ends a key is just a key.
            self.streak = 0;
        }
        self.flashes.retain(|&(_, _, t)| time - t < FLASH_SECS);
        self.flashes.push((key, hit, time));
        hit
    }
}

/// Started this close after a click (in beats), the count-in counts that click.
const ON_CLICK: f64 = 0.25;

/// The metronome beat the count-in starts from, started at beat `m`.
fn first_count(m: f64) -> f64 {
    let m = m.max(0.0);
    let click = m.floor();
    if m - click <= ON_CLICK { click } else { click + 1.0 }
}

/// How far from its beat a note still counts, in beats.
fn window(bpm: f64) -> f64 {
    (ON_TIME_SECS * bpm / 60.0).min(0.45)
}

const FLASH_SECS: f64 = 0.35;

pub const BACKGROUND: Color32 = Color32::from_rgb(24, 24, 30);
const OCTAVE_LINE: Color32 = Color32::from_rgb(52, 52, 62);
const FIFTH_LINE: Color32 = Color32::from_rgb(36, 36, 44);
const BEAT_LINE: Color32 = Color32::from_rgb(34, 34, 42);
const HIT_LINE: Color32 = Color32::from_rgb(190, 190, 205);
const HIT: Color32 = Color32::from_rgb(74, 222, 128);
const WRONG: Color32 = Color32::from_rgb(239, 68, 68);
const MISSED: Color32 = Color32::from_rgb(78, 78, 88);

/// Draws `song` falling into `area` at the song's `beat`, over the key columns `spans` (from
/// [`crate::keyboard::key_spans`]); the keys are just below the area's bottom edge. `run` gives
/// each note's status; `time` (the window's clock, seconds) fades the flashes.
#[expect(clippy::too_many_arguments, reason = "one drawing call with its inputs")]
pub fn draw(
    painter: &egui::Painter,
    area: Rect,
    spans: &[(u8, f32, f32)],
    song: &Song,
    run: Option<&Run>,
    beat: f64,
    bpm: f64,
    time: f64,
) {
    painter.rect_filled(area, 0.0, BACKGROUND);
    let line = area.bottom();
    let shown = (SECONDS_SHOWN * bpm / 60.0).max(3.0);
    let per_beat = f64::from(area.height()) / shown;
    // Screen y of a song beat.
    let y = |b: f64| {
        let v = (beat - b).mul_add(per_beat, f64::from(line));
        #[expect(clippy::cast_possible_truncation, reason = "screen coordinates")]
        #[expect(clippy::as_conversions, reason = "no as-free f64 to f32 conversion")]
        let v = v.clamp(-1e6, 1e6) as f32;
        v
    };
    let span = |key: u8| spans.iter().find(|&&(k, _, _)| k == key).map(|&(_, l, r)| (l, r));

    // Lines between octaves (B|C) and at E|F, like the gaps between groups of black keys.
    for &(key, left, _) in spans {
        let colour = match key % 12 {
            0 => OCTAVE_LINE,
            5 => FIFTH_LINE,
            _ => continue,
        };
        painter.vline(left, area.y_range(), Stroke::new(1.0, colour));
    }
    // A faint line on every beat.
    let first = beat.floor();
    for i in 0..=200u32 {
        let b = first + f64::from(i);
        if b > beat + shown + 1.0 {
            break;
        }
        painter.hline(area.x_range(), y(b), Stroke::new(1.0, BEAT_LINE));
    }

    for (i, n) in song.notes.iter().enumerate() {
        if n.beat > beat + shown + 0.1 {
            break;
        }
        if n.beat + n.len < beat - 0.1 {
            continue;
        }
        let Some((left, right)) = span(n.key) else { continue };
        let pad = ((right - left) * 0.08).max(1.0);
        let rect = Rect::from_min_max(
            pos2(left + pad, y(n.beat + n.len) + 1.0),
            pos2(right - pad, y(n.beat) - 1.0),
        );
        let hand = match n.hand {
            Hand::Left => LEFT_HAND,
            Hand::Right => RIGHT_HAND,
        };
        let base = if is_black(n.key) {
            hand.lerp_to_gamma(Color32::BLACK, 0.3)
        } else {
            hand
        };
        let status = run.map_or(Status::Coming, |r| r.status(i));
        let (fill, edge) = match status {
            Status::Coming => (base, base.lerp_to_gamma(Color32::WHITE, 0.35)),
            Status::Hit => (base.lerp_to_gamma(HIT, 0.55), Color32::WHITE),
            Status::Missed => (MISSED, hand.lerp_to_gamma(MISSED, 0.4)),
        };
        let radius = (rect.width() * 0.25).min(6.0);
        painter.rect_filled(rect, radius, fill);
        painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
        // At the bottom, where it lands: the finger (scales), and the note's name above it.
        if rect.width() >= 10.0 {
            let size = (rect.width() * 0.62).clamp(10.5, 16.0);
            let name_size = (rect.width() * 0.5).clamp(9.5, 14.0);
            let mut bottom = rect.bottom() - 3.0;
            if let Some(finger) = n.finger.filter(|_| rect.height() >= 14.0) {
                painter.text(
                    pos2(rect.center().x, bottom),
                    Align2::CENTER_BOTTOM,
                    finger.to_string(),
                    FontId::proportional(size),
                    Color32::WHITE,
                );
                bottom -= size + 2.0;
            }
            if bottom - name_size >= rect.top() + 2.0 {
                painter.text(
                    pos2(rect.center().x, bottom),
                    Align2::CENTER_BOTTOM,
                    n.name(),
                    FontId::proportional(name_size),
                    Color32::WHITE.gamma_multiply(0.85),
                );
            }
        }
    }

    // The line the notes fall onto, lit where a key was just played.
    painter.hline(area.x_range(), line - 1.5, Stroke::new(3.0, HIT_LINE));
    for &(key, hit, at) in run.map_or(&[][..], |r| r.flashes.as_slice()) {
        let age = time - at;
        let Some((left, right)) = span(key).filter(|_| (0.0..FLASH_SECS).contains(&age)) else {
            continue;
        };
        #[expect(clippy::cast_possible_truncation, reason = "an opacity")]
        #[expect(clippy::as_conversions, reason = "no as-free f64 to f32 conversion")]
        let fade = (1.0 - age / FLASH_SECS) as f32;
        let colour = if hit { HIT } else { WRONG };
        let glow = Rect::from_min_max(pos2(left, (-40.0f32).mul_add(fade, line)), pos2(right, line));
        painter.rect_filled(glow, 2.0, colour.gamma_multiply(0.35 * fade));
        painter.hline(left..=right, line - 1.5, Stroke::new(4.0, colour.gamma_multiply(fade)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c_major() -> Song {
        Song::scale(
            Scale {
                root: 0,
                kind: Kind::Major,
            },
            Exercise::default(),
        )
    }

    #[test]
    fn a_scale_goes_up_and_down_two_octaves_hands_together_an_octave_apart() {
        let s = c_major();
        let right: Vec<u8> = s
            .notes
            .iter()
            .filter(|n| n.hand == Hand::Right)
            .map(|n| n.key)
            .collect();
        assert_eq!(right.len(), 29);
        assert_eq!(right.first(), Some(&60));
        assert_eq!(right.get(14), Some(&84), "the top, once");
        assert_eq!(right.get(15), Some(&83));
        assert_eq!(right.last(), Some(&60));
        let left_first = s.notes.iter().find(|n| n.hand == Hand::Left).unwrap();
        assert_eq!((left_first.key, left_first.beat, left_first.finger), (48, 0.0, Some(5)));
        assert!((s.end() - 30.0).abs() < 1e-9, "the last note held two beats");
    }

    /// A one-track format-0 file: tempo 100 BPM, 480 ticks a beat, C4 and E4 a beat each with
    /// a beat's rest first, and a drum note that is left out.
    fn small_midi() -> Vec<u8> {
        let track = vec![
            0x00, 0xFF, 0x51, 0x03, 0x09, 0x27, 0xC0, // tempo 600 000 µs: 100 BPM
            0x83, 0x60, 0x90, 60, 100, // after 480 ticks: C4 on
            0x00, 0x99, 36, 100, // drum
            0x83, 0x60, 0x80, 60, 0, // C4 off
            0x00, 0x90, 64, 90, // E4 on
            0x83, 0x60, 0x90, 64, 0, // E4 off (velocity 0)
            0x00, 0xFF, 0x2F, 0x00,
        ];
        midi_file(&track)
    }

    /// A one-track MIDI file, 480 ticks a beat, holding `track`'s events.
    fn midi_file(track: &[u8]) -> Vec<u8> {
        let mut f = b"MThd\0\0\0\x06\0\0\0\x01\x01\xE0MTrk".to_vec();
        f.extend_from_slice(&u32::try_from(track.len()).unwrap().to_be_bytes());
        f.extend_from_slice(track);
        f
    }

    #[test]
    fn a_midi_file_gives_its_notes_on_their_beats() {
        let s = Song::from_midi(&small_midi(), "test").unwrap();
        assert_eq!(s.bpm, Some(100));
        let notes: Vec<(u8, f64, f64)> = s.notes.iter().map(|n| (n.key, n.beat, n.len)).collect();
        // Starting on the beat of the first note.
        assert_eq!(notes, [(60, 0.0, 1.0), (64, 1.0, 1.0)]);
        assert!(s.notes.iter().all(|n| n.hand == Hand::Right), "middle C and up");
        assert_eq!(s.notes.iter().map(|n| n.velocity).collect::<Vec<_>>(), [100, 90]);
        assert!(Song::from_midi(b"nope", "x").is_err());
    }

    #[test]
    fn the_notes_to_play_as_the_song_moves_on() {
        let s = c_major();
        // Beat 0: both hands start; nothing ends.
        let (starts, ends) = s.between(-0.5, 0.0);
        assert_eq!(starts, [(48, 80), (60, 80)]);
        assert_eq!(ends, [0u8; 0], "nothing ends");
        // Beat 1: the first notes end as the next ones start.
        let (starts, ends) = s.between(0.9, 1.0);
        assert_eq!(starts, [(50, 80), (62, 80)]);
        assert_eq!(ends, [48, 60]);
        assert_eq!(s.between(0.2, 0.8), (Vec::new(), Vec::new()));
    }

    #[test]
    fn notes_are_named_as_their_scale_writes_them() {
        let names = |root, kind| {
            let s = Song::scale(Scale { root, kind }, Exercise::default());
            let mut up: Vec<String> = s
                .notes
                .iter()
                .filter(|n| n.hand == Hand::Right)
                .take(8)
                .map(Note::name)
                .collect();
            up.dedup();
            up.join(" ")
        };
        assert_eq!(names(6, Kind::Major), "F# G# A# B C# D# E# F#");
        assert_eq!(names(1, Kind::Major), "Db Eb F Gb Ab Bb C Db");
        assert_eq!(names(9, Kind::HarmonicMinor), "A B C D E F G# A");
        // Melodic minor comes down as natural minor: G, F going down from A.
        let s = Song::scale(
            Scale {
                root: 9,
                kind: Kind::MelodicMinor,
            },
            Exercise::default(),
        );
        let right: Vec<String> = s
            .notes
            .iter()
            .filter(|n| n.hand == Hand::Right)
            .map(Note::name)
            .collect();
        assert_eq!(right.get(5..7), Some(&["F#".to_owned(), "G#".to_owned()][..]));
        assert_eq!(right.get(15..17), Some(&["G".to_owned(), "F".to_owned()][..]));
    }

    #[test]
    fn a_songs_notes_follow_its_key_signature() {
        // small_midi with Bb (A#, key 70) added, and then a key signature of two flats.
        let song = |signature: Option<u8>| {
            let mut track = Vec::new();
            if let Some(flats) = signature {
                track.extend_from_slice(&[0x00, 0xFF, 0x59, 0x02, flats, 0x00]);
            }
            track.extend_from_slice(&[0x00, 0x90, 70, 100, 0x83, 0x60, 0x80, 70, 0, 0x00, 0xFF, 0x2F, 0x00]);
            Song::from_midi(&midi_file(&track), "t").unwrap().notes[0].name()
        };
        assert_eq!(song(None), "Bb", "the usual name without a signature");
        let key = |b: &[u8]| {
            let mut track = vec![0x00, 0xFF, 0x59, 0x02];
            track.extend_from_slice(b);
            track.extend_from_slice(&[0x00, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0, 0x00, 0xFF, 0x2F, 0x00]);
            Song::from_midi(&midi_file(&track), "t").unwrap().key.map(Key::name)
        };
        assert_eq!(key(&[0xFE, 0]).as_deref(), Some("Bb major"));
        // Each key's scale has the key's name.
        for sharps in -7..=7 {
            for minor in [false, true] {
                let k = Key { sharps, minor };
                let enharmonic = matches!(
                    k.name().as_str(),
                    "Cb major" | "C# major" | "A# minor" | "D# minor" | "Ab minor" | "Gb major"
                );
                if !enharmonic {
                    let scale = k.scale().name().replace(" natural minor", " minor");
                    assert_eq!(scale, k.name(), "{sharps} sharps, minor {minor}");
                }
            }
        }
        assert_eq!(key(&[0x03, 1]).as_deref(), Some("F# minor"));
        assert_eq!(key(&[0x00, 0]).as_deref(), Some("C major"));
        assert_eq!(
            Song::from_midi(&small_midi(), "t").unwrap().key,
            None,
            "none in the file"
        );
        assert_eq!(song(Some(0xFE)), "Bb", "two flats");
        assert_eq!(song(Some(0x02)), "A#", "two sharps");
    }

    #[test]
    fn hands_come_from_track_names_or_pitch() {
        let p = |name: &str, key| Part {
            name: name.to_owned(),
            notes: vec![Played {
                key,
                beat: 0.0,
                len: 1.0,
                velocity: 64,
            }],
        };
        assert_eq!(
            assign_hands(&[p("Piano", 40), p("Piano", 70)]),
            [Some(Hand::Left), Some(Hand::Right)]
        );
        assert_eq!(
            assign_hands(&[p("Right Hand", 40), p("LH", 70)]),
            [Some(Hand::Right), Some(Hand::Left)]
        );
        assert_eq!(assign_hands(&[p("Piano", 40)]), [None]);
    }

    #[test]
    fn playing_along_counts_notes_on_time_and_misses_late_ones() {
        let mut r = Run::new(c_major(), false);
        let bpm = 60.0; // a beat a second: on time within 0.15 beats
        r.advance(None, 0.0, bpm);
        assert!((r.beat() + COUNT_IN).abs() < 1e-9, "waits for the metronome");
        // The metronome clicked beat 0 just now: the song starts four beats on.
        r.advance(Some(0.0), 0.0, bpm);
        r.advance(Some(4.1), 0.0, bpm);
        assert!(r.press(60, None, 0.0, bpm, 0.0), "right hand C4, a little late");
        assert!(!r.press(62, None, 0.0, bpm, 0.0), "D4 is not due yet");
        assert_eq!(r.streak, 0, "a wrong key ends the streak");
        r.advance(Some(4.9), 0.0, bpm);
        assert!(r.press(62, None, 0.0, bpm, 0.0), "D4, a little early");
        r.advance(Some(6.0), 0.0, bpm);
        // The left hand's first two notes were never played.
        assert_eq!(r.status(0), Status::Missed);
        assert_eq!(r.score(), (2, 58));
    }

    #[test]
    fn waiting_stops_at_each_chord_until_it_is_played() {
        let mut r = Run::new(c_major(), true);
        r.advance(None, 10.0, 60.0);
        assert!(r.beat().abs() < 1e-9, "stops at the first notes");
        assert!(r.press(60, None, 0.0, 60.0, 0.0));
        r.advance(None, 10.0, 60.0);
        assert!(r.beat().abs() < 1e-9, "the left hand's C3 is still to play");
        assert!(r.press(48, None, 0.0, 60.0, 0.0));
        r.advance(None, 0.5, 60.0);
        assert!((r.beat() - 0.5).abs() < 1e-9, "on again");
        assert!(r.press(62, None, 0.0, 60.0, 0.0), "the next chord, played early");
        assert_eq!(r.best_streak, 3);
    }

    #[test]
    fn evenness_is_the_spread_of_the_timing_and_loudness() {
        let mut r = Run::new(c_major(), false);
        let bpm = 60.0;
        // Play along at 60 BPM: a beat is a second. Each beat's two notes (left, right).
        let plays: [(f64, f64, u8, u8); 4] = [
            // (left offset, right offset in seconds, left velocity, right velocity)
            (0.00, 0.02, 70, 80),
            (0.01, 0.03, 72, 78),
            (-0.01, 0.01, 68, 82),
            (0.00, 0.02, 70, 80),
        ];
        let (left, right) = (r_notes(&r, Hand::Left), r_notes(&r, Hand::Right));
        // The metronome's first click at 0: the song's beat 0 is the metronome's beat 4.
        r.advance(Some(0.0), 0.0, bpm);
        for (beat, (lo, ro, lv, rv)) in plays.iter().enumerate() {
            let b = f64::from(u32::try_from(beat).unwrap());
            // The frame comes after both presses; each press is `ago` beats before it.
            let now = b + lo.max(*ro) + 0.005;
            r.advance(Some(now + COUNT_IN), 0.0, bpm);
            assert!(r.press(left[beat], Some(*lv), now - (b + lo), bpm, 0.0));
            assert!(r.press(right[beat], Some(*rv), now - (b + ro), bpm, 0.0));
        }
        let e = r.evenness();
        assert_eq!(e.played, 8);
        // Offsets 0, 20, 10, 30, -10, 10, 0, 20 ms: mean 10, spread about 12.2.
        assert!((e.lean_ms.unwrap() - 10.0).abs() < 0.5, "{e:?}");
        assert!((e.spread_ms.unwrap() - 12.25).abs() < 0.5, "{e:?}");
        // The right hand 20 ms after the left on every beat.
        assert!((e.hands_apart_ms.unwrap() - 20.0).abs() < 0.5, "{e:?}");
        assert!(e.loudness.unwrap() > 4.0 && e.loudness.unwrap() < 6.0, "{e:?}");
        let texts: Vec<String> = e.gauges().into_iter().map(|g| g.text).collect();
        assert_eq!(texts, ["±12 ms", "10 ms late", "±5", "20 ms apart"]);
    }

    #[test]
    fn evenness_needs_two_notes_and_says_nothing_it_cannot_know() {
        let r = Run::new(c_major(), true);
        let e = r.evenness();
        assert_eq!((e.spread_ms, e.loudness, e.hands_apart_ms), (None, None, None));
        assert_eq!(e.gauges().len(), 0);
        assert_eq!((e.played, e.notes), (0, 58));
    }

    fn r_notes(r: &Run, hand: Hand) -> Vec<u8> {
        r.song.notes.iter().filter(|n| n.hand == hand).map(|n| n.key).collect()
    }

    #[test]
    fn the_count_in_is_four_clicks_however_the_run_is_started() {
        // Switched on with the run: the first click is the first count.
        assert!((first_count(0.0) - 0.0).abs() < 1e-9);
        assert!((first_count(-0.02) - 0.0).abs() < 1e-9, "a click about to be heard");
        // Already ticking, started on a click (a chord played in time): that click counts.
        assert!((first_count(12.03) - 12.0).abs() < 1e-9);
        // Started between clicks (Space at any moment): from the next one.
        assert!((first_count(12.5) - 13.0).abs() < 1e-9);
        assert!((first_count(12.9) - 13.0).abs() < 1e-9);
    }
}
