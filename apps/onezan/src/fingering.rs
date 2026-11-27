//! Scale fingerings, for display on the keyboard.
//!
//! A hand-written table (MIDI cannot tell which finger played a note, so the app shows
//! fingerings but cannot check them): major, natural, harmonic and melodic minor in all twelve
//! keys, both hands, any number of octaves. It is the traditional fingering, cross-checked against
//! several published charts. The tests hold their numbers verbatim and list every place where this
//! table differs from one of them, and why:
//!
//! - pianoscales.org: major, natural and harmonic minor, one octave, both hands, all keys.
//! - Hoffman Academy, "Melodic Minor Scale Fingering Guide" (PDF): two octaves, both hands, all keys.
//! - Greg Spero, composition.gregspero.com/lessons/a3: natural minor, all keys, and the melodic
//!   minor exceptions (F# and C# right hand, G# left hand).
//! - musicandhealth.co.uk: B, F# and C# major, two octaves.
//! - University of Miami (Prof. Escadon's chart, via cs.miami.edu/~burt): where the fourth finger
//!   falls in every key and scale type.
//!
//! Melodic minor goes up with a raised sixth and seventh and comes down as natural minor, with
//! natural minor's fingering (the classical way; all sources agree).

/// The scale types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Major,
    NaturalMinor,
    HarmonicMinor,
    MelodicMinor,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::Major, Self::NaturalMinor, Self::HarmonicMinor, Self::MelodicMinor];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::NaturalMinor => "natural minor",
            Self::HarmonicMinor => "harmonic minor",
            Self::MelodicMinor => "melodic minor",
        }
    }

    /// The scale played in `dir`: melodic minor comes down as natural minor.
    const fn played(self, dir: Direction) -> Self {
        match (self, dir) {
            (Self::MelodicMinor, Direction::Down) => Self::NaturalMinor,
            (k, _) => k,
        }
    }

    /// The seven degrees in semitones above the root.
    const fn steps(self) -> [u8; 7] {
        match self {
            Self::Major => [0, 2, 4, 5, 7, 9, 11],
            Self::NaturalMinor => [0, 2, 3, 5, 7, 8, 10],
            Self::HarmonicMinor => [0, 2, 3, 5, 7, 8, 11],
            Self::MelodicMinor => [0, 2, 3, 5, 7, 9, 11],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

/// A scale: its root as a pitch class (0 = C, 1 = C#/Db, ... 11 = B) and its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    pub root: u8,
    pub kind: Kind,
}

/// Roots in the suggested learning order: C, G, D, A, E (one fingering); B and F; the black-key
/// scales Db and Gb; then Bb, Eb, Ab. The same order for each scale type.
pub const LEARNING_ORDER: [u8; 12] = [0, 7, 2, 9, 4, 11, 5, 1, 6, 10, 3, 8];

impl Scale {
    /// Every scale: by type, then in the learning order.
    pub fn all() -> impl Iterator<Item = Self> {
        Kind::ALL
            .into_iter()
            .flat_map(|kind| LEARNING_ORDER.into_iter().map(move |root| Self { root, kind }))
    }

    /// For example "Db major" or "F# harmonic minor": the usual spelling of each key.
    pub fn name(self) -> String {
        format!("{} {}", self.root_name(), self.kind.name())
    }

    /// The letter (0 = C) `key` is written with in this scale: each degree has its own letter
    /// (F# major's seventh is E#, not F). A key outside the scale: as out of context.
    pub fn letter(self, key: u8) -> u8 {
        let root_letter = crate::chords::LETTERS
            .iter()
            .position(|l| self.root_name().starts_with(l))
            .and_then(|i| u8::try_from(i).ok())
            .unwrap_or(0);
        let step = (key % 12).wrapping_add(12).wrapping_sub(self.root % 12) % 12;
        // Up and down (melodic minor comes down as natural minor), the same letters per degree.
        [Direction::Up, Direction::Down]
            .into_iter()
            .find_map(|d| self.kind.played(d).steps().iter().position(|&s| s == step))
            .and_then(|degree| u8::try_from(degree).ok())
            .map_or_else(
                || crate::chords::letter_of(key % 12, None),
                |degree| root_letter.wrapping_add(degree) % 7,
            )
    }

    /// The root as the key is written: "Db", "F#", "C#".
    fn root_name(self) -> &'static str {
        const MAJOR: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
        const MINOR: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B"];
        let names = if self.kind == Kind::Major { MAJOR } else { MINOR };
        names.get(usize::from(self.root % 12)).copied().unwrap_or("?")
    }

    /// The key signature: the number of sharps (positive) or flats (negative). Every minor
    /// scale is written with the natural minor's signature; harmonic and melodic minor raise
    /// their sixth and seventh with accidentals in the music, not in the signature.
    pub fn key_signature(self) -> i8 {
        // Steps round the circle of fifths from C: F -1, G 1, ...; a sharp adds 7, a flat takes 7.
        let mut chars = self.root_name().chars();
        let letter: i8 = match chars.next() {
            Some('F') => -1,
            Some('G') => 1,
            Some('D') => 2,
            Some('A') => 3,
            Some('E') => 4,
            Some('B') => 5,
            _ => 0,
        };
        let accidental: i8 = match chars.next() {
            Some('#') => 7,
            Some('b') => -7,
            _ => 0,
        };
        // A minor key has three fifths fewer than its tonic major (A minor is C major's signature).
        let minor: i8 = if self.kind == Kind::Major { 0 } else { -3 };
        letter.saturating_add(accidental).saturating_add(minor)
    }

    /// Where each hand starts for two octaves hands together, as (left, right) MIDI keys: the
    /// right hand on the root nearest middle C, the left hand two octaves lower, so the hands
    /// meet on that root (C major: left hand C2 to C4, right hand C4 to C6).
    pub const fn hand_roots(self) -> (u8, u8) {
        let root = self.root % 12;
        let right = if root <= 6 { 60_u8 } else { 48 }.saturating_add(root);
        (right.saturating_sub(24), right)
    }

    /// Whether the pitch class of `key` is in the scale (going up).
    pub fn contains(self, key: u8) -> bool {
        let step = (key % 12).wrapping_add(12).wrapping_sub(self.root % 12) % 12;
        self.kind.steps().contains(&step)
    }

    /// The notes of `hand` playing the scale in `dir` over `octaves` from the root at `root_key`
    /// (a MIDI key), as (key, finger), lowest key first whichever the direction. Notes above
    /// key 127 are left out.
    pub fn notes(self, hand: Hand, root_key: u8, octaves: u8, dir: Direction) -> Vec<(u8, u8)> {
        let kind = self.kind.played(dir);
        let steps = kind.steps();
        let fingering = table(kind, self.root % 12);
        let p = match hand {
            Hand::Left => &fingering.left,
            Hand::Right => &fingering.right,
        };
        let count = usize::from(octaves).saturating_mul(7).saturating_add(1);
        let high_from = count.saturating_sub(p.high.len());
        (0..count)
            .filter_map(|i| {
                let octave = u8::try_from(i / 7).ok()?;
                let key = octave
                    .checked_mul(12)?
                    .checked_add(*steps.get(i % 7)?)?
                    .checked_add(root_key)?;
                let finger = i
                    .checked_sub(high_from)
                    .map_or_else(|| p.low.get(i).or_else(|| p.cycle.get(i % 7)), |j| p.high.get(j));
                (key <= 127).then_some((key, *finger?))
            })
            .collect()
    }
}

/// One hand's fingering of a scale, for any number of octaves, written going up.
struct Pattern {
    /// The finger on each degree, root first, inside the scale (the root between two octaves).
    cycle: [u8; 7],
    /// Fingers for the lowest notes, in place of the cycle's: the left hand's 5 on the bottom root.
    low: &'static [u8],
    /// Fingers for the highest notes, lowest first, in place of the cycle's: the right hand's 5 on
    /// the top root.
    high: &'static [u8],
}

struct Fingering {
    right: Pattern,
    left: Pattern,
}

const fn hand(cycle: [u8; 7], low: &'static [u8], high: &'static [u8]) -> Pattern {
    Pattern { cycle, low, high }
}

const fn both(right: Pattern, left: Pattern) -> Fingering {
    Fingering { right, left }
}

// Shapes shared by several keys.
/// Right hand 1 2 3 1 2 3 4 (5 on top): C, G, D, A, E and B major, and their minors.
const RH_C: Pattern = hand([1, 2, 3, 1, 2, 3, 4], &[], &[5]);
/// Left hand (5) 4 3 2 1 3 2 1: C, G, D, A, E and F, major and minor.
const LH_C: Pattern = hand([1, 4, 3, 2, 1, 3, 2], &[5], &[]);
/// Right hand 1 2 3 4 1 2 3 (4 on top): F major and minor.
const RH_F: Pattern = hand([1, 2, 3, 4, 1, 2, 3], &[], &[4]);
/// Left hand (4) 3 2 1 4 3 2 1: B major and minor.
const LH_B: Pattern = hand([1, 3, 2, 1, 4, 3, 2], &[4], &[]);
/// Left hand 3 2 1 4 3 2 1, thumb on the scale's 3rd and 7th: Db, Eb, Ab, Bb major, C# minor,
/// and G# harmonic and melodic minor.
const LH_3214: Pattern = hand([3, 2, 1, 4, 3, 2, 1], &[], &[]);
/// Right hand 3 4 1 2 3 1 2: Ab major, C# and G# minor.
const RH_34: Pattern = hand([3, 4, 1, 2, 3, 1, 2], &[], &[]);
/// Right hand 4 1 2 3 1 2 3, starting with 2 on the bottom Bb: Bb major and minor.
const RH_BB: Pattern = hand([4, 1, 2, 3, 1, 2, 3], &[2], &[]);
/// Right hand 3 1 2 3 4 1 2: Eb major and minor.
const RH_EB: Pattern = hand([3, 1, 2, 3, 4, 1, 2], &[], &[]);
/// Right hand 2 3 1 2 3 4 1: Db major, and F# and C# melodic minor going up (their raised 6th
/// would fall under the thumb with the natural minor's fingering).
const RH_23123: Pattern = hand([2, 3, 1, 2, 3, 4, 1], &[], &[]);

const C_SHAPE: Fingering = both(RH_C, LH_C);

const MAJOR: [Fingering; 12] = [
    C_SHAPE,                 // C
    both(RH_23123, LH_3214), // Db
    C_SHAPE,                 // D
    both(RH_EB, LH_3214),    // Eb
    C_SHAPE,                 // E
    both(RH_F, LH_C),        // F
    both(
        hand([2, 3, 4, 1, 2, 3, 1], &[], &[]),
        hand([4, 3, 2, 1, 3, 2, 1], &[], &[]),
    ), // F#/Gb
    C_SHAPE,                 // G
    both(RH_34, LH_3214),    // Ab
    C_SHAPE,                 // A
    both(RH_BB, LH_3214),    // Bb
    both(RH_C, LH_B),        // B
];

/// F# minor's right hand starts 2 3 (both one-octave charts) and goes on 3 4 1 in later octaves.
const RH_F_SHARP_MINOR: Pattern = hand([3, 4, 1, 2, 3, 1, 2], &[2, 3], &[]);
const LH_F_SHARP: Pattern = hand([4, 3, 2, 1, 3, 2, 1], &[], &[]);
const LH_E_FLAT_MINOR: Pattern = hand([2, 1, 4, 3, 2, 1, 3], &[], &[]);
const LH_B_FLAT_MINOR: Pattern = hand([2, 1, 3, 2, 1, 4, 3], &[], &[]);

const NATURAL_MINOR: [Fingering; 12] = [
    C_SHAPE,                                            // C
    both(RH_34, LH_3214),                               // C#
    C_SHAPE,                                            // D
    both(RH_EB, LH_E_FLAT_MINOR),                       // Eb/D#
    C_SHAPE,                                            // E
    both(RH_F, LH_C),                                   // F
    both(RH_F_SHARP_MINOR, LH_F_SHARP),                 // F#
    C_SHAPE,                                            // G
    both(RH_34, hand([3, 2, 1, 3, 2, 1, 4], &[], &[])), // G#: 4 on F#
    C_SHAPE,                                            // A
    both(RH_BB, LH_B_FLAT_MINOR),                       // Bb
    both(RH_C, LH_B),                                   // B
];

/// As natural minor, except G#: its raised 7th (F##, a white key) takes the thumb.
const HARMONIC_MINOR: [Fingering; 12] = [
    C_SHAPE,
    both(RH_34, LH_3214),
    C_SHAPE,
    both(RH_EB, LH_E_FLAT_MINOR),
    C_SHAPE,
    both(RH_F, LH_C),
    both(RH_F_SHARP_MINOR, LH_F_SHARP),
    C_SHAPE,
    both(RH_34, LH_3214), // G#
    C_SHAPE,
    both(RH_BB, LH_B_FLAT_MINOR),
    both(RH_C, LH_B),
];

/// Going up. As harmonic minor, except the right hand of F# and C#.
const MELODIC_MINOR: [Fingering; 12] = [
    C_SHAPE,
    both(RH_23123, LH_3214), // C#
    C_SHAPE,
    both(RH_EB, LH_E_FLAT_MINOR),
    C_SHAPE,
    both(RH_F, LH_C),
    both(RH_23123, LH_F_SHARP), // F#
    C_SHAPE,
    both(RH_34, LH_3214),
    C_SHAPE,
    both(RH_BB, LH_B_FLAT_MINOR),
    both(RH_C, LH_B),
];

fn table(kind: Kind, root: u8) -> &'static Fingering {
    let keys = match kind {
        Kind::Major => &MAJOR,
        Kind::NaturalMinor => &NATURAL_MINOR,
        Kind::HarmonicMinor => &HARMONIC_MINOR,
        Kind::MelodicMinor => &MELODIC_MINOR,
    };
    keys.get(usize::from(root)).unwrap_or(&C_SHAPE)
}

/// Which hands play a scale exercise, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hands {
    /// Both hands in the same direction, an octave apart (the left hand lower).
    #[default]
    Together,
    Right,
    Left,
    /// From the key-note, both thumbs on it, the hands moving apart and back.
    Contrary,
}

impl Hands {
    pub const ALL: [Self; 4] = [Self::Together, Self::Right, Self::Left, Self::Contrary];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Together => "hands together",
            Self::Right => "right hand",
            Self::Left => "left hand",
            Self::Contrary => "contrary motion",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.name() == name)
    }
}

/// How a scale is practised: which hands, in what motion, over how many octaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exercise {
    pub hands: Hands,
    pub octaves: u8,
}

impl Default for Exercise {
    fn default() -> Self {
        Self {
            hands: Hands::Together,
            octaves: 2,
        }
    }
}

/// One hand's part of an exercise: it covers `octaves` from the root at `low` (a MIDI key), and
/// goes `first` that way from its starting root, then back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    pub hand: Hand,
    pub low: u8,
    pub first: Direction,
}

/// The keys a part may use: an 88-key piano, A0 to C8.
const PIANO: std::ops::RangeInclusive<u8> = 21..=108;

impl Exercise {
    /// The most octaves; contrary motion goes up to 2 (both hands spread from the middle).
    pub const MAX_OCTAVES: u8 = 4;

    /// The octaves played: 1 to [`Self::MAX_OCTAVES`], at most 2 in contrary motion.
    pub fn octaves(self) -> u8 {
        let max = if self.hands == Hands::Contrary {
            2
        } else {
            Self::MAX_OCTAVES
        };
        self.octaves.clamp(1, max)
    }

    /// Each hand's part, placed around middle C: the right hand from the root nearest middle C
    /// (the left hand an octave below it, or ending on it alone); contrary motion from that root
    /// in both hands. Moved by whole octaves to stay on an 88-key piano.
    pub fn parts(self, s: Scale) -> Vec<Part> {
        let n = self.octaves();
        let span = n.saturating_mul(12);
        let root = s.hand_roots().1;
        let part = |hand, low, first| Part { hand, low, first };
        let mut parts = match self.hands {
            Hands::Together => vec![
                part(Hand::Left, root.saturating_sub(12), Direction::Up),
                part(Hand::Right, root, Direction::Up),
            ],
            Hands::Right => vec![part(Hand::Right, root, Direction::Up)],
            Hands::Left => vec![part(Hand::Left, root.saturating_sub(span), Direction::Up)],
            Hands::Contrary => vec![
                part(Hand::Left, root.saturating_sub(span), Direction::Down),
                part(Hand::Right, root, Direction::Up),
            ],
        };
        let lowest = parts.iter().map(|p| p.low).min().unwrap_or(root);
        let highest = parts.iter().map(|p| p.low.saturating_add(span)).max().unwrap_or(root);
        // Whole octaves down (over the top) or up (under the bottom), in keys.
        let octaves_in = |over: u8| over.div_ceil(12).saturating_mul(12);
        let down = octaves_in(highest.saturating_sub(*PIANO.end()));
        let up = octaves_in(PIANO.start().saturating_sub(lowest));
        for p in &mut parts {
            p.low = p.low.saturating_sub(down).saturating_add(up);
        }
        parts
    }

    /// Each part's notes in the order played, as (hand, key, finger): from its starting root
    /// `first` way over the octaves and back, the turning note once.
    pub fn lines(self, s: Scale) -> Vec<(Hand, Vec<(u8, u8)>)> {
        let n = self.octaves();
        self.parts(s)
            .into_iter()
            .map(|p| {
                let up = s.notes(p.hand, p.low, n, Direction::Up);
                let down = s.notes(p.hand, p.low, n, Direction::Down);
                let line: Vec<(u8, u8)> = match p.first {
                    Direction::Up => up.iter().chain(down.iter().rev().skip(1)).copied().collect(),
                    Direction::Down => down.iter().rev().chain(up.iter().skip(1)).copied().collect(),
                };
                (p.hand, line)
            })
            .collect()
    }
}

/// Which fingering is shown on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preview {
    #[default]
    Off,
    /// Two octaves hands together, going up: the right hand from the root nearest middle C, the
    /// left hand from two octaves lower, so the hands meet on that root (C major: left hand C2 to
    /// C4, right hand C4 to C6).
    Scale(Scale),
}

impl Preview {
    pub fn all() -> impl Iterator<Item = Self> {
        std::iter::once(Self::Off).chain(Scale::all().map(Self::Scale))
    }

    /// The preview called `name` (as [`Self::name`] gives it), if there is one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().find(|p| p.name() == name)
    }

    pub fn name(self) -> String {
        match self {
            Self::Off => "none".to_owned(),
            Self::Scale(s) => s.name(),
        }
    }

    /// What the keyboard shows for `exercise`: each hand's fingering going out from its start
    /// (up, or down for the left hand in contrary motion).
    pub fn shown(self, exercise: Exercise) -> Shown {
        let mut shown = Shown {
            left: [None; 128],
            right: [None; 128],
            scale: None,
        };
        if let Self::Scale(s) = self {
            shown.scale = Some(s);
            for p in exercise.parts(s) {
                let slots = match p.hand {
                    Hand::Left => &mut shown.left,
                    Hand::Right => &mut shown.right,
                };
                for (key, finger) in s.notes(p.hand, p.low, exercise.octaves(), p.first) {
                    if let Some(slot) = slots.get_mut(usize::from(key)) {
                        *slot = Some(finger);
                    }
                }
            }
        }
        shown
    }
}

/// A preview's badges and tint, per key.
pub struct Shown {
    left: [Option<u8>; 128],
    right: [Option<u8>; 128],
    scale: Option<Scale>,
}

impl Shown {
    /// The finger of each hand on `key`: (left, right).
    pub fn fingers(&self, key: u8) -> (Option<u8>, Option<u8>) {
        let i = usize::from(key);
        (
            self.left.get(i).copied().flatten(),
            self.right.get(i).copied().flatten(),
        )
    }

    /// Whether `key` belongs to the scale shown.
    pub fn in_scale(&self, key: u8) -> bool {
        self.scale.is_some_and(|s| s.contains(key))
    }

    /// The keys with a badge, so the keyboard can show them all.
    pub fn keys(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=127u8).filter(|&k| self.fingers(k) != (None, None))
    }
}

#[cfg(test)]
#[expect(clippy::arithmetic_side_effects, reason = "small key numbers in tests")]
mod tests {
    use super::*;

    fn scale(kind: Kind, root: u8) -> Scale {
        Scale { root, kind }
    }

    fn fingers(s: Scale, hand: Hand, octaves: u8, dir: Direction) -> String {
        let notes = s.notes(hand, 48 + s.root, octaves, dir);
        notes.iter().map(|(_, f)| f.to_string()).collect::<Vec<_>>().join(" ")
    }

    fn root_of(name: &str) -> u8 {
        let names = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B"];
        let alt = ["C", "Db", "D", "D#", "E", "F", "Gb", "G", "Ab", "A", "A#", "B"];
        let i = names
            .iter()
            .position(|&n| n == name)
            .or_else(|| alt.iter().position(|&n| n == name));
        u8::try_from(i.unwrap()).unwrap()
    }

    /// Checks one source's table: (kind, root, RH, LH) rows, with `octaves`, against ours.
    /// `differ` lists (kind, root, hand) where we differ on purpose; those must actually differ.
    fn check(rows: &[(Kind, &str, &str, &str)], octaves: u8, differ: &[(Kind, &str, Hand)]) {
        for &(kind, root, rh, lh) in rows {
            let s = scale(kind, root_of(root));
            for (hand, theirs) in [(Hand::Right, rh), (Hand::Left, lh)] {
                let ours = fingers(s, hand, octaves, Direction::Up);
                if differ.contains(&(kind, root, hand)) {
                    assert_ne!(ours, theirs, "{} {hand:?}: listed as differing", s.name());
                } else {
                    assert_eq!(ours, theirs, "{} {hand:?}", s.name());
                }
            }
        }
    }

    use Kind::{HarmonicMinor as H, Major as M, MelodicMinor as Mel, NaturalMinor as N};

    /// pianoscales.org, one octave going up, copied from its per-key lines.
    #[test]
    fn matches_pianoscales_org() {
        let rows = [
            (M, "C", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (M, "D", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (M, "E", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (M, "F", "1 2 3 4 1 2 3 4", "5 4 3 2 1 3 2 1"),
            (M, "G", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (M, "A", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (M, "B", "1 2 3 1 2 3 4 5", "4 3 2 1 4 3 2 1"),
            (M, "Db", "2 3 1 2 3 4 1 2", "3 2 1 4 3 2 1 3"),
            (M, "Eb", "3 1 2 3 4 1 2 3", "3 2 1 4 3 2 1 3"),
            (M, "F#", "2 3 4 1 2 3 1 2", "4 3 2 1 3 2 1 4"),
            (M, "Ab", "3 4 1 2 3 1 2 3", "3 2 1 4 3 2 1 3"),
            (M, "Bb", "2 1 2 3 1 2 3 4", "3 2 1 4 3 2 1 3"),
            (H, "A", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (H, "Bb", "2 1 2 3 1 2 3 4", "2 1 3 2 1 4 3 2"),
            (H, "B", "1 2 3 1 2 3 4 5", "4 3 2 1 4 3 2 1"),
            (H, "C", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (H, "C#", "3 4 1 2 3 1 2 3", "3 2 1 4 3 2 1 3"),
            (H, "D", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (H, "Eb", "3 1 2 3 4 1 2 3", "2 1 4 3 2 1 3 2"),
            (H, "E", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (H, "F", "1 2 3 4 1 2 3 4", "5 4 3 2 1 3 2 1"),
            (H, "F#", "2 3 1 2 3 1 2 3", "4 3 2 1 3 2 1 4"),
            (H, "G", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (H, "G#", "2 3 1 2 3 1 2 3", "3 2 1 4 3 2 1 3"),
            (N, "A", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "Bb", "2 1 2 3 1 2 3 4", "2 1 3 2 1 4 3 2"),
            (N, "B", "1 2 3 1 2 3 4 5", "4 3 2 1 4 3 2 1"),
            (N, "C", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "C#", "3 4 1 2 3 1 2 3", "3 2 1 4 3 2 1 3"),
            (N, "D", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "Eb", "3 1 2 3 4 1 2 3", "2 1 4 3 2 1 3 2"),
            (N, "E", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "F", "1 2 3 4 1 2 3 4", "5 4 3 2 1 3 2 1"),
            (N, "F#", "2 3 1 2 3 1 2 3", "4 3 2 1 3 2 1 4"),
            (N, "G", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "G#", "3 4 1 2 3 1 2 3", "3 2 1 3 2 1 4 3"),
        ];
        // G# harmonic minor's right hand: we start 3 4 like its natural minor on the same site,
        // Greg Spero, and Hoffman's G# melodic minor (the notes under the hand are the same).
        check(&rows, 1, &[(H, "G#", Hand::Right)]);
    }

    /// Greg Spero: natural minor, one octave; harmonic and melodic the same except where noted.
    #[test]
    fn matches_greg_spero() {
        let natural = [
            (N, "A", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "E", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "B", "1 2 3 1 2 3 4 5", "4 3 2 1 4 3 2 1"),
            (N, "F#", "2 3 1 2 3 1 2 3", "4 3 2 1 3 2 1 4"),
            (N, "C#", "3 4 1 2 3 1 2 3", "3 2 1 4 3 2 1 3"),
            (N, "G#", "3 4 1 2 3 1 2 3", "3 2 1 3 2 1 4 3"),
            (N, "Eb", "3 1 2 3 4 1 2 3", "2 1 4 3 2 1 3 2"),
            (N, "Bb", "2 1 2 3 1 2 3 4", "2 1 3 2 1 4 3 2"),
            (N, "F", "1 2 3 4 1 2 3 4", "5 4 3 2 1 3 2 1"),
            (N, "C", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "G", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
            (N, "D", "1 2 3 1 2 3 4 5", "5 4 3 2 1 3 2 1"),
        ];
        check(&natural, 1, &[]);
        let harmonic: Vec<_> = natural.iter().map(|&(_, r, rh, lh)| (H, r, rh, lh)).collect();
        // G#: its left hand takes the thumb on F## (page's own table).
        check(&harmonic, 1, &[(H, "G#", Hand::Left)]);
        assert_eq!(fingers(scale(H, 8), Hand::Left, 1, Direction::Up), "3 2 1 4 3 2 1 3");
        let exceptions = [
            (Mel, "F#", "2 3 1 2 3 4 1 2", "4 3 2 1 3 2 1 4"),
            // The page starts on 3 4; we start on 2 3 like Hoffman's two-octave chart. Same
            // fingers from the second note on (3 4 1 2 3 4 1 2 against 2 3 1 2 3 4 1 2).
            (Mel, "C#", "3 4 1 2 3 4 1 2", "3 2 1 4 3 2 1 3"),
            (Mel, "G#", "3 4 1 2 3 1 2 3", "3 2 1 4 3 2 1 3"),
        ];
        check(&exceptions, 1, &[(Mel, "C#", Hand::Right)]);
    }

    /// Hoffman Academy's melodic minor guide, two octaves going up (read from the PDF's layout).
    #[test]
    fn matches_hoffman_melodic_minor() {
        let rows = [
            (
                Mel,
                "A",
                "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5",
                "5 4 3 2 1 3 2 1 4 3 2 1 3 2 1",
            ),
            (
                Mel,
                "E",
                "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5",
                "5 4 3 2 1 3 2 1 4 3 2 1 3 2 1",
            ),
            (
                Mel,
                "B",
                "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5",
                "4 3 2 1 4 3 2 1 3 2 1 4 3 2 1",
            ),
            (
                Mel,
                "F#",
                "2 3 1 2 3 4 1 2 3 1 2 3 4 1 2",
                "4 3 2 1 3 2 1 4 3 2 1 3 2 1 2",
            ),
            (
                Mel,
                "C#",
                "2 3 1 2 3 4 1 2 3 1 2 3 4 1 2",
                "3 2 1 4 3 2 1 3 2 1 4 3 2 1 3",
            ),
            (
                Mel,
                "G#",
                "3 4 1 2 3 1 2 3 4 1 2 3 1 2 3",
                "3 2 1 4 3 2 1 3 2 1 4 3 2 1 3",
            ),
            (
                Mel,
                "Eb",
                "3 1 2 3 4 1 2 3 1 2 3 4 1 2 3",
                "2 1 4 3 2 1 3 2 1 4 3 2 1 3 2",
            ),
            (
                Mel,
                "Bb",
                "4 1 2 3 1 2 3 4 1 2 3 1 2 3 4",
                "2 1 3 2 1 4 3 2 1 3 2 1 4 3 2",
            ),
            (
                Mel,
                "F",
                "1 2 3 4 1 2 3 1 2 3 4 1 2 3 4",
                "5 4 3 2 1 3 2 1 4 3 2 1 3 2 1",
            ),
            (
                Mel,
                "C",
                "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5",
                "5 4 3 2 1 3 2 1 4 3 2 1 3 2 1",
            ),
            (
                Mel,
                "G",
                "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5",
                "5 4 3 2 1 3 2 1 4 3 2 1 3 2 1",
            ),
            (
                Mel,
                "D",
                "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5",
                "5 4 3 2 1 3 2 1 4 3 2 1 3 2 1",
            ),
        ];
        // F# left hand: Hoffman ends on 2 at the top F#; we keep 4, like every one-octave chart
        // of F# (pianoscales.org, Greg Spero). Bb right hand: Hoffman starts on 4; we start on 2
        // like pianoscales.org and Greg Spero for Bb natural minor, which the scale comes down as.
        check(&rows, 2, &[(Mel, "F#", Hand::Left), (Mel, "Bb", Hand::Right)]);
        assert_eq!(
            fingers(scale(Mel, 6), Hand::Left, 2, Direction::Up),
            "4 3 2 1 3 2 1 4 3 2 1 3 2 1 4"
        );
        assert_eq!(
            fingers(scale(Mel, 10), Hand::Right, 2, Direction::Up),
            "2 1 2 3 1 2 3 4 1 2 3 1 2 3 4"
        );
    }

    /// musicandhealth.co.uk, two octaves.
    #[test]
    fn matches_musicandhealth() {
        let rows = [
            (M, "B", "1 2 3 1 2 3 4 1 2 3 1 2 3 4 5", "4 3 2 1 4 3 2 1 3 2 1 4 3 2 1"),
            (
                M,
                "F#",
                "2 3 4 1 2 3 1 2 3 4 1 2 3 1 2",
                "4 3 2 1 3 2 1 4 3 2 1 3 2 1 2",
            ),
            (
                M,
                "Db",
                "2 3 1 2 3 4 1 2 3 1 2 3 4 1 2",
                "3 2 1 4 3 2 1 3 2 1 4 3 2 1 2",
            ),
        ];
        // The left hand's top note (F#, Db): the page ends on 2, "the nearest available finger";
        // we keep the one-octave charts' 4 and 3.
        check(&rows, 2, &[(M, "F#", Hand::Left), (M, "Db", Hand::Left)]);
    }

    /// The Miami chart: where the fourth finger falls, as pitch classes (left hand, right hand).
    #[test]
    fn fourth_finger_matches_miami_chart() {
        let fourth = |s: Scale, hand| -> Vec<u8> {
            // Three octaves, without the end notes (the 4 on B major's bottom note is a start).
            let notes = s.notes(hand, 36 + s.root, 3, Direction::Up);
            let mut v: Vec<u8> = notes[2..notes.len() - 1]
                .iter()
                .copied()
                .filter(|&(_, f)| f == 4)
                .map(|(k, _)| k % 12)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        // C, D, E, G, A in every type: left hand on the 2nd degree, right hand on the 7th.
        for kind in Kind::ALL {
            for root in [0u8, 2, 4, 7, 9] {
                let s = scale(kind, root);
                assert_eq!(fourth(s, Hand::Left), [(root + 2) % 12], "{}", s.name());
                let seventh = (root + kind.steps()[6]) % 12;
                assert_eq!(fourth(s, Hand::Right), [seventh], "{}", s.name());
            }
        }
        // (kind, root, left, right), from the chart's rows for B, Db, Eb, Gb, Ab and Bb.
        let rows: [(Kind, u8, &[u8], &[u8]); 20] = [
            (M, 11, &[6], &[10]),
            (N, 11, &[6], &[9]),
            (H, 11, &[6], &[10]),
            (M, 1, &[6], &[10]),
            (H, 1, &[6], &[3]),
            (Mel, 1, &[6], &[10]),
            (M, 3, &[8], &[10]),
            (N, 3, &[6], &[10]),
            (H, 3, &[6], &[10]),
            (Mel, 3, &[6], &[10]),
            (M, 6, &[6], &[10]),
            (N, 6, &[6], &[8]),
            (H, 6, &[6], &[8]),
            (Mel, 6, &[6], &[3]),
            (M, 8, &[1], &[10]),
            (N, 8, &[6], &[10]),
            (H, 8, &[1], &[10]),
            (M, 10, &[3], &[10]),
            (N, 10, &[6], &[10]),
            (H, 10, &[6], &[10]),
        ];
        for (kind, root, left, right) in rows {
            let s = scale(kind, root);
            assert_eq!(fourth(s, Hand::Left), left, "{} left", s.name());
            assert_eq!(fourth(s, Hand::Right), right, "{} right", s.name());
        }
    }

    /// Rules every fingering keeps, in every key, both directions, one to four octaves.
    #[test]
    fn fingerings_are_playable() {
        for s in Scale::all() {
            for dir in [Direction::Up, Direction::Down] {
                for octaves in 1..=4 {
                    for hand in [Hand::Left, Hand::Right] {
                        let notes = s.notes(hand, 24 + s.root, octaves, dir);
                        let what = format!("{} {hand:?} {dir:?} {octaves}", s.name());
                        assert_eq!(notes.len(), usize::from(octaves) * 7 + 1, "{what}");
                        for &(key, f) in &notes {
                            assert!((1..=5).contains(&f), "{what}");
                            assert!(
                                !(f == 1 && crate::keyboard::is_black(key)),
                                "{what}: thumb on a black key"
                            );
                        }
                        for w in notes.windows(2) {
                            let ((_, a), (_, b)) = (w[0], w[1]);
                            // Going up, the right hand moves to a higher finger or passes the thumb
                            // under; the left hand to a lower finger or crosses over the thumb.
                            let ok = match hand {
                                Hand::Right => b > a || b == 1,
                                Hand::Left => b < a || a == 1,
                            };
                            assert!(ok, "{what}: {a} then {b}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn melodic_minor_comes_down_as_natural_minor() {
        let s = scale(Mel, 9);
        let up: Vec<u8> = s.notes(Hand::Right, 57, 1, Direction::Up).iter().map(|n| n.0).collect();
        let down: Vec<u8> = s
            .notes(Hand::Right, 57, 1, Direction::Down)
            .iter()
            .map(|n| n.0)
            .collect();
        assert_eq!(up, [57, 59, 60, 62, 64, 66, 68, 69]);
        assert_eq!(down, [57, 59, 60, 62, 64, 65, 67, 69]);
        // F# melodic: 2 3 1 2 3 4 1 2 up; natural minor's 3 at the top coming down (Hoffman).
        let f_sharp = scale(Mel, 6);
        assert_eq!(fingers(f_sharp, Hand::Right, 1, Direction::Down), "2 3 1 2 3 1 2 3");
    }

    #[test]
    fn previews_are_found_by_name() {
        for p in Preview::all() {
            assert_eq!(Preview::from_name(&p.name()), Some(p));
        }
        assert_eq!(Preview::from_name("H major"), None);
    }

    #[test]
    fn names_and_spelling() {
        assert_eq!(scale(M, 1).name(), "Db major");
        assert_eq!(scale(H, 1).name(), "C# harmonic minor");
        assert_eq!(Scale::all().count(), 48);
        assert!(scale(H, 8).contains(67)); // G# harmonic minor's F## is G
    }

    #[test]
    fn key_signatures() {
        // Sharps positive, flats negative, as the keys are named.
        let major = [
            (0, 0),
            (7, 1),
            (2, 2),
            (9, 3),
            (4, 4),
            (11, 5),
            (6, 6),
            (5, -1),
            (10, -2),
        ];
        let major = major.into_iter().chain([(3, -3), (8, -4), (1, -5)]);
        for (root, sig) in major {
            assert_eq!(scale(M, root).key_signature(), sig, "{}", scale(M, root).name());
        }
        // Minor: the relative major's signature, the same for all three minor types.
        let minor = [
            (9, 0),
            (4, 1),
            (11, 2),
            (6, 3),
            (1, 4),
            (8, 5),
            (2, -1),
            (7, -2),
            (0, -3),
        ];
        let minor = minor.into_iter().chain([(5, -4), (10, -5), (3, -6)]);
        for (root, sig) in minor {
            for kind in [Kind::NaturalMinor, H, Kind::MelodicMinor] {
                assert_eq!(scale(kind, root).key_signature(), sig, "{}", scale(kind, root).name());
            }
        }
    }

    fn ex(hands: Hands, octaves: u8) -> Exercise {
        Exercise { hands, octaves }
    }

    #[test]
    fn hands_together_play_an_octave_apart_from_the_root_nearest_middle_c() {
        let shown = Preview::Scale(scale(M, 0)).shown(Exercise::default());
        // Left hand C3 to C5, right hand C4 to C6.
        assert_eq!(shown.fingers(48), (Some(5), None));
        assert_eq!(shown.fingers(60), (Some(1), Some(1)));
        assert_eq!(shown.fingers(72), (Some(1), Some(1)));
        assert_eq!(shown.fingers(84), (None, Some(5)));
        assert_eq!(shown.fingers(61), (None, None));
        assert!(!shown.in_scale(61));
        assert_eq!((shown.keys().min(), shown.keys().max()), (Some(48), Some(84)));
        // G major: from the G below middle C.
        let g = Preview::Scale(scale(M, 7)).shown(Exercise::default());
        assert_eq!(g.fingers(55), (Some(1), Some(1)));
        assert_eq!((g.keys().min(), g.keys().max()), (Some(43), Some(79)));
    }

    #[test]
    fn contrary_motion_starts_on_one_key_note_with_both_thumbs() {
        let c = scale(M, 0);
        let shown = Preview::Scale(c).shown(ex(Hands::Contrary, 2));
        assert_eq!(shown.fingers(60), (Some(1), Some(1)));
        assert_eq!(shown.fingers(36), (Some(5), None));
        assert_eq!(shown.fingers(84), (None, Some(5)));
        let lines = ex(Hands::Contrary, 2).lines(c);
        let line = |hand| lines.iter().find(|(h, _)| *h == hand).map(|(_, l)| l.clone()).unwrap();
        let (left, right) = (line(Hand::Left), line(Hand::Right));
        assert_eq!(left.iter().take(3).map(|n| n.0).collect::<Vec<_>>(), [60, 59, 57]);
        assert_eq!(right.iter().take(3).map(|n| n.0).collect::<Vec<_>>(), [60, 62, 64]);
        // Out and back, the far note once: 2 × 14 + 1 notes, ending where they began.
        assert_eq!((left.len(), right.len()), (29, 29));
        assert_eq!(
            (left.last().map(|n| n.0), right.last().map(|n| n.0)),
            (Some(60), Some(60))
        );
        // At most two octaves.
        assert_eq!(ex(Hands::Contrary, 4).octaves(), 2);
    }

    #[test]
    fn one_hand_alone_plays_from_or_to_middle_c() {
        let c = scale(M, 0);
        let parts = |hands, n| ex(hands, n).parts(c);
        assert_eq!(
            parts(Hands::Right, 2),
            [Part {
                hand: Hand::Right,
                low: 60,
                first: Direction::Up
            }]
        );
        assert_eq!(
            parts(Hands::Left, 2),
            [Part {
                hand: Hand::Left,
                low: 36,
                first: Direction::Up
            }]
        );
    }

    #[test]
    fn exercises_stay_on_an_88_key_piano() {
        let lows = |e: Exercise, s: Scale| e.parts(s).iter().map(|p| p.low).collect::<Vec<_>>();
        // Four octaves of C fit as they are (the right hand ends on C8).
        assert_eq!(lows(ex(Hands::Together, 4), scale(M, 0)), [48, 60]);
        // F#: an octave lower, so the right hand ends on F#7, not above the piano.
        assert_eq!(lows(ex(Hands::Together, 4), scale(M, 6)), [42, 54]);
        // The left hand alone over four octaves: an octave higher, from C1.
        assert_eq!(lows(ex(Hands::Left, 4), scale(M, 0)), [24]);
        for e in [
            ex(Hands::Together, 4),
            ex(Hands::Right, 4),
            ex(Hands::Left, 4),
            ex(Hands::Contrary, 2),
        ] {
            for s in Scale::all() {
                for (_, line) in e.lines(s) {
                    assert!(line.iter().all(|(k, _)| PIANO.contains(k)), "{} {e:?}", s.name());
                    assert_eq!(line.len(), usize::from(e.octaves()) * 14 + 1, "{} {e:?}", s.name());
                }
            }
        }
    }

    #[test]
    fn hands_have_names_that_read_back() {
        for h in Hands::ALL {
            assert_eq!(Hands::from_name(h.name()), Some(h));
        }
    }
}
