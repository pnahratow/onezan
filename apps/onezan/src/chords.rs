//! Chords: what the held keys are called, and a fingering for them.
//!
//! **Naming.** The held keys are reduced to their pitch classes; every pitch class is tried as
//! the root against a table of chord types (triads, sixths, sevenths, ninths, sus and add
//! chords). Seventh and ninth chords also match without their fifth, as they are often played.
//! When several readings fit (C6 and Am7 are the same notes), the one whose root is in the bass
//! wins, then a full chord over one without its fifth, then the simpler type (the table's order).
//! A chord over a bass other than its root is written with a slash (C/E) and named by its
//! inversion. Notes are spelled from the root (the third of E major is G#, not Ab).
//!
//! **Fingering.** There is no official standard for block-chord fingering: the exam boards' books
//! (ABRSM, Trinity, RCM) print broken chords and arpeggios, and teachers differ. So the table
//! holds only what the sources agree on, and shows nothing where they differ or where fewer than
//! two of them give a fingering. Major and minor triads in close position (three notes within an
//! octave) and triads with the bottom note doubled an octave up (four notes, as in C E G C), in
//! each inversion. Nothing for other chords (diminished, augmented, sevenths, ...), wider
//! voicings, or a hand holding a single note or an interval. Sources, read 2026-11-25:
//!
//! - "Piano Chords in Inversions", appendix 16.2 of a Salvation Army music manual
//!   (music.saconnects.org, engraved score): triads and four-note chords in all 24 keys, both
//!   hands. The same fingering in every key.
//! - musicandhealth.co.uk (a teacher's scale and broken chord syllabus in the ABRSM style):
//!   four-note broken chords key by key (C, D, A, E, Eb, Ab, Db, B, Bb, F# major; F, C, G, F#, C#,
//!   G#, B, Bb, Eb minor), both hands; its left hand depends on the key's black keys.
//! - Hoffman Academy, "Chord Inversions" (chart and text) and Piano with Lauren, "Chord Inversion
//!   Fingerings" (a teacher's chart): triads, both hands, any key.
//! - Yamaha, "Basic piano chords for beginners": major and minor triads in root position.
//!
//! Where they differ (so nothing is shown): the right hand's second inversion triad (1 3 5:
//! Hoffman, Lauren; 1 2 4: Salvation Army); the left hand's first inversion minor triad (5 3 1:
//! Hoffman, Lauren; 4 2 1: Salvation Army); four notes, right hand, second inversion major
//! (1 2 3 5: Salvation Army; 1 2 4 5: musicandhealth); left hand root position minor (5 3 2 1 /
//! 5 4 2 1) and major in C (5 3 2 1 / 5 4 2 1); left hand first inversion F#, C# and
//! G# minor (5 4 2 1 / 5 3 2 1). A key musicandhealth does not cover gets its fingering only
//! where musicandhealth gives the same one in every key it covers. When a source offers an
//! alternative ("1245 or 1235"), its first choice counts. The tests hold the sources' numbers
//! and work out from them what may be shown.
//!
//! Which hand plays is not guessed: keys within an octave get both hands' fingerings, where
//! known. Wider, the keys are split at the biggest gap each hand can span: the lower part is the
//! left hand's, the upper part the right hand's.

use crate::fingering::Hand;

/// One chord type: its symbol after the root, its name, and its tones as (semitones above the
/// root, steps above the root's letter), so the notes can be spelled.
struct Kind {
    symbol: &'static str,
    name: &'static str,
    tones: &'static [(u8, u8)],
}

const fn kind(symbol: &'static str, name: &'static str, tones: &'static [(u8, u8)]) -> Kind {
    Kind { symbol, name, tones }
}

// Tones, as (semitones, letter steps).
const R: (u8, u8) = (0, 0);
const M2: (u8, u8) = (2, 1);
const MIN3: (u8, u8) = (3, 2);
const MAJ3: (u8, u8) = (4, 2);
const P4: (u8, u8) = (5, 3);
const DIM5: (u8, u8) = (6, 4);
const P5: (u8, u8) = (7, 4);
const AUG5: (u8, u8) = (8, 4);
const M6: (u8, u8) = (9, 5);
const DIM7: (u8, u8) = (9, 6);
const MIN7: (u8, u8) = (10, 6);
const MAJ7: (u8, u8) = (11, 6);

/// The chord types, simplest first (that order breaks ties).
const KINDS: [Kind; 20] = [
    kind("", "major", &[R, MAJ3, P5]),
    kind("m", "minor", &[R, MIN3, P5]),
    kind("dim", "diminished", &[R, MIN3, DIM5]),
    kind("aug", "augmented", &[R, MAJ3, AUG5]),
    kind("sus4", "suspended fourth", &[R, P4, P5]),
    kind("sus2", "suspended second", &[R, M2, P5]),
    kind("7", "dominant seventh", &[R, MAJ3, P5, MIN7]),
    kind("maj7", "major seventh", &[R, MAJ3, P5, MAJ7]),
    kind("m7", "minor seventh", &[R, MIN3, P5, MIN7]),
    kind("m7b5", "half-diminished seventh", &[R, MIN3, DIM5, MIN7]),
    kind("dim7", "diminished seventh", &[R, MIN3, DIM5, DIM7]),
    kind("6", "major sixth", &[R, MAJ3, P5, M6]),
    kind("m6", "minor sixth", &[R, MIN3, P5, M6]),
    kind("mMaj7", "minor-major seventh", &[R, MIN3, P5, MAJ7]),
    kind("7sus4", "dominant seventh suspended fourth", &[R, P4, P5, MIN7]),
    kind("add9", "added ninth", &[R, M2, MAJ3, P5]),
    kind("madd9", "minor added ninth", &[R, M2, MIN3, P5]),
    kind("9", "dominant ninth", &[R, M2, MAJ3, P5, MIN7]),
    kind("maj9", "major ninth", &[R, M2, MAJ3, P5, MAJ7]),
    kind("m9", "minor ninth", &[R, M2, MIN3, P5, MIN7]),
];

impl Kind {
    /// Whether it may be played without its fifth: sevenths and ninths with a perfect fifth.
    fn fifth_optional(&self) -> bool {
        self.tones.len() >= 4 && self.tones.contains(&P5) && !self.symbol.contains("sus")
    }

    /// Spelled from a minor third (C#m rather than Dbm, as minor keys are).
    fn minorish(&self) -> bool {
        self.tones.contains(&MIN3)
    }
}

/// What the held keys are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Chord(Chord),
    /// Keys that make no chord: their names, lowest first, and for two pitch classes (or one in
    /// octaves) the interval between them.
    Notes {
        notes: String,
        interval: Option<&'static str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    /// For example "C/E".
    pub symbol: String,
    /// For example "C major, first inversion".
    pub name: String,
    /// The chord's notes as played, lowest first, each pitch class once: "E G C".
    pub notes: String,
}

/// What `keys` (MIDI keys, any order) are; `None` for no keys.
#[must_use]
pub fn read(keys: &[u8]) -> Option<Reading> {
    let mut keys = keys.to_vec();
    keys.sort_unstable();
    keys.dedup();
    let bass = keys.first().map(|k| k % 12)?;
    // Pitch classes in the order played, lowest first.
    let mut played: Vec<u8> = Vec::new();
    for pc in keys.iter().map(|k| k % 12) {
        if !played.contains(&pc) {
            played.push(pc);
        }
    }
    if played.len() >= 3
        && let Some(chord) = best_chord(&played, bass)
    {
        return Some(Reading::Chord(chord));
    }
    let notes = played.iter().map(|&pc| default_name(pc)).collect::<Vec<_>>().join(" ");
    let interval = match played.as_slice() {
        [_] if keys.len() > 1 => Some("octave"),
        [low, high] => INTERVALS
            .get(usize::from(high.wrapping_add(12).wrapping_sub(*low) % 12))
            .copied(),
        _ => None,
    };
    Some(Reading::Notes { notes, interval })
}

const INTERVALS: [&str; 12] = [
    "unison",
    "minor second",
    "major second",
    "minor third",
    "major third",
    "perfect fourth",
    "tritone",
    "perfect fifth",
    "minor sixth",
    "major sixth",
    "minor seventh",
    "major seventh",
];

/// The best reading of `played` (pitch classes, lowest first) over `bass`.
fn best_chord(played: &[u8], bass: u8) -> Option<Chord> {
    let mut best: Option<((bool, bool, usize), u8, &Kind)> = None;
    for &root in played {
        let mut rel: Vec<u8> = played
            .iter()
            .map(|pc| pc.wrapping_add(12).wrapping_sub(root) % 12)
            .collect();
        rel.sort_unstable();
        for (i, k) in KINDS.iter().enumerate() {
            let full: Vec<u8> = k.tones.iter().map(|t| t.0).collect();
            let without_fifth: Vec<u8> = full.iter().copied().filter(|&s| s != P5.0).collect();
            let omitted = if rel == full {
                false
            } else if k.fifth_optional() && rel == without_fifth {
                true
            } else {
                continue;
            };
            let rank = (root != bass, omitted, i);
            if best.as_ref().is_none_or(|(r, _, _)| rank < *r) {
                best = Some((rank, root, k));
            }
        }
    }
    let (_, root, k) = best?;
    Some(chord(played, bass, root, k))
}

fn chord(played: &[u8], bass: u8, root: u8, k: &Kind) -> Chord {
    let (letter, _) = root_spelling(root, k.minorish());
    let root_name = spell(letter, root);
    let tone = |pc: u8| {
        let semis = pc.wrapping_add(12).wrapping_sub(root) % 12;
        k.tones.iter().find(|t| t.0 == semis).copied()
    };
    let name_of = |pc: u8| tone(pc).map_or_else(|| default_name(pc), |t| spell(letter.wrapping_add(t.1) % 7, pc));
    let notes = played.iter().map(|&pc| name_of(pc)).collect::<Vec<_>>().join(" ");
    let mut symbol = format!("{root_name}{}", k.symbol);
    let mut name = format!("{root_name} {}", k.name);
    if bass != root {
        let bass_name = name_of(bass);
        symbol = format!("{symbol}/{bass_name}");
        let position = match tone(bass).map(|t| t.1) {
            Some(2) => "first inversion".to_owned(),
            Some(4) => "second inversion".to_owned(),
            Some(6) => "third inversion".to_owned(),
            _ => format!("{bass_name} in the bass"),
        };
        name = format!("{name}, {position}");
    }
    Chord { symbol, name, notes }
}

/// The natural pitch class of each letter, C to B.
const NATURAL: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const LETTERS: [&str; 7] = ["C", "D", "E", "F", "G", "A", "B"];

/// The letter (0 = C) a root is spelled with, and its name: flats for major-ish chords (Db, Eb,
/// Ab, Bb; F#), sharps where minor keys use them (C#, F#, G#).
fn root_spelling(pc: u8, minorish: bool) -> (u8, String) {
    const MAJOR: [u8; 12] = [0, 1, 1, 2, 2, 3, 3, 4, 5, 5, 6, 6];
    const MINOR: [u8; 12] = [0, 0, 1, 2, 2, 3, 3, 4, 4, 5, 6, 6];
    let table = if minorish { MINOR } else { MAJOR };
    let letter = table.get(usize::from(pc % 12)).copied().unwrap_or(0);
    (letter, spell(letter, pc))
}

/// `pc` written with `letter` and the accidentals it needs ("Fb", "G##"); a name without them
/// when it would need more than two.
fn spell(letter: u8, pc: u8) -> String {
    let natural = NATURAL.get(usize::from(letter)).copied().unwrap_or(0);
    let accidental = match pc.wrapping_add(12).wrapping_sub(natural) % 12 {
        0 => "",
        1 => "#",
        2 => "##",
        11 => "b",
        10 => "bb",
        _ => return default_name(pc),
    };
    format!(
        "{}{accidental}",
        LETTERS.get(usize::from(letter)).copied().unwrap_or("?")
    )
}

/// A note's name out of context: the major key spellings (Db, Eb, F#, Ab, Bb).
fn default_name(pc: u8) -> String {
    root_spelling(pc, false).1
}

/// A fingering for `keys` held together, as (key, hand, finger): only where the sources agree,
/// so often for one hand or none (see the module's notes).
#[must_use]
pub fn fingering(keys: &[u8]) -> Vec<(u8, Hand, u8)> {
    let mut keys = keys.to_vec();
    keys.sort_unstable();
    keys.dedup();
    if Shape::of(&keys).is_some() {
        return [Hand::Left, Hand::Right]
            .into_iter()
            .flat_map(|hand| on(&keys, hand))
            .collect();
    }
    // Two hands: split at the biggest gap each side can span (the lower of equal gaps).
    let mut splits: Vec<(u8, usize)> = keys
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| Some((w.get(1)?.saturating_sub(*w.first()?), i.saturating_add(1))))
        .collect();
    splits.sort_by_key(|&(gap, at)| (std::cmp::Reverse(gap), at));
    splits
        .into_iter()
        .map(|(_, at)| keys.split_at(at))
        .find(|(low, high)| spannable(low) && spannable(high))
        .map(|(low, high)| on(low, Hand::Left).into_iter().chain(on(high, Hand::Right)).collect())
        .unwrap_or_default()
}

/// Whether one hand can hold `keys` (sorted): five at most, within an octave.
fn spannable(keys: &[u8]) -> bool {
    let span = keys.last().zip(keys.first()).map_or(0, |(h, l)| h.saturating_sub(*l));
    !keys.is_empty() && keys.len() <= 5 && span <= 12
}

/// `hand`'s fingers on `keys` (sorted), if they are a shape the sources agree on.
fn on(keys: &[u8], hand: Hand) -> Vec<(u8, Hand, u8)> {
    Shape::of(keys)
        .and_then(|s| s.fingers(hand))
        .map(|f| keys.iter().zip(f).map(|(&key, &finger)| (key, hand, finger)).collect())
        .unwrap_or_default()
}

/// A major or minor triad in close position, alone or with its bottom note an octave up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    minor: bool,
    /// The root's pitch class.
    root: u8,
    /// 0 root position, 1 first, 2 second inversion.
    inversion: u8,
    /// Four notes: the bottom one doubled an octave up.
    doubled: bool,
}

impl Shape {
    /// The shape of `keys` (sorted, no repeats), if they make one.
    fn of(keys: &[u8]) -> Option<Self> {
        let low = *keys.first()?;
        let rel: Vec<u8> = keys.iter().map(|k| k.saturating_sub(low)).collect();
        let (third, top, doubled) = match rel.as_slice() {
            [0, a, b] => (*a, *b, false),
            [0, a, b, 12] => (*a, *b, true),
            _ => return None,
        };
        // (minor, inversion, the root's distance above the bass)
        let (minor, inversion, root) = match (third, top) {
            (4, 7) => (false, 0, 0),
            (3, 7) => (true, 0, 0),
            (3, 8) => (false, 1, 8),
            (4, 9) => (true, 1, 9),
            (5, 9) => (false, 2, 5),
            (5, 8) => (true, 2, 5),
            _ => return None,
        };
        Some(Self {
            minor,
            root: low.wrapping_add(root) % 12,
            inversion,
            doubled,
        })
    }

    /// `hand`'s fingers, lowest key first, where the sources agree.
    fn fingers(self, hand: Hand) -> Option<&'static [u8]> {
        // Pitch classes (C = 0).
        const C_F_G: [u8; 3] = [0, 5, 7];
        const D_A_E_FS_CS_GS: [u8; 6] = [2, 9, 4, 6, 1, 8];
        let Self {
            minor,
            root,
            inversion,
            doubled,
        } = self;
        match (doubled, hand, inversion) {
            (false, Hand::Right, 0) => Some(&[1, 3, 5]),
            (false, Hand::Right, 1) => Some(&[1, 2, 5]),
            (false, Hand::Left, 0) => Some(&[5, 3, 1]),
            (false, Hand::Left, 1) if !minor => Some(&[5, 3, 1]),
            (false, Hand::Left, 2) => Some(&[5, 2, 1]),
            (true, Hand::Right, 0) => Some(&[1, 2, 3, 5]),
            (true, Hand::Right, 1) => Some(&[1, 2, 4, 5]),
            (true, Hand::Right, 2) if minor => Some(&[1, 2, 3, 5]),
            (true, Hand::Left, 0) if !minor && !C_F_G.contains(&root) => Some(&[5, 3, 2, 1]),
            (true, Hand::Left, 1) if !(minor && D_A_E_FS_CS_GS.contains(&root)) => Some(&[5, 4, 2, 1]),
            (true, Hand::Left, 2) => Some(&[5, 3, 2, 1]),
            _ => None,
        }
    }
}

#[cfg(test)]
#[expect(clippy::arithmetic_side_effects, reason = "small key numbers in tests")]
mod tests {
    use super::*;

    const C4: u8 = 60;

    fn chord(keys: &[u8]) -> Chord {
        match read(keys) {
            Some(Reading::Chord(c)) => c,
            other => panic!("{keys:?}: no chord: {other:?}"),
        }
    }

    fn symbol(keys: &[u8]) -> String {
        chord(keys).symbol
    }

    /// Keys from note names, lowest first, each above the last, starting in octave 4.
    fn keys(names: &str) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for n in names.split_whitespace() {
            let letter = LETTERS.iter().position(|l| n.starts_with(l)).unwrap();
            let mut pc = i32::from(NATURAL[letter]);
            for c in n.chars().skip(1) {
                pc += if c == '#' { 1 } else { -1 };
            }
            let pc = u8::try_from(pc.rem_euclid(12)).unwrap();
            let mut key = C4 + pc;
            while out.last().is_some_and(|&l| key <= l) {
                key += 12;
            }
            out.push(key);
        }
        out
    }

    #[test]
    fn triads_and_their_inversions() {
        assert_eq!(symbol(&keys("C E G")), "C");
        assert_eq!(chord(&keys("C E G")).name, "C major");
        assert_eq!(symbol(&keys("E G C")), "C/E");
        assert_eq!(chord(&keys("E G C")).name, "C major, first inversion");
        assert_eq!(chord(&keys("G C E")).name, "C major, second inversion");
        assert_eq!(symbol(&keys("A C E")), "Am");
        assert_eq!(symbol(&keys("B D F")), "Bdim");
        assert_eq!(symbol(&keys("C E G#")), "Caug");
        assert_eq!(symbol(&keys("C F G")), "Csus4");
        // Doubled notes and wide spacing change nothing.
        assert_eq!(symbol(&[36, 48, 64, 67, 72]), "C");
    }

    #[test]
    fn notes_are_spelled_from_the_root() {
        assert_eq!(chord(&keys("E G# B")).notes, "E G# B");
        assert_eq!(chord(&keys("Db F Ab")).notes, "Db F Ab");
        assert_eq!(chord(&keys("C# E G#")).symbol, "C#m");
        assert_eq!(chord(&keys("Eb Gb Bb")).notes, "Eb Gb Bb");
        assert_eq!(chord(&keys("C Eb Gb A")).notes, "C Eb Gb Bbb");
        assert_eq!(symbol(&keys("G# B E")), "E/G#");
    }

    #[test]
    fn sevenths_ninths_and_the_missing_fifth() {
        assert_eq!(symbol(&keys("G B D F")), "G7");
        assert_eq!(chord(&keys("B D F G")).name, "G dominant seventh, first inversion");
        assert_eq!(chord(&keys("F G B D")).name, "G dominant seventh, third inversion");
        assert_eq!(symbol(&keys("C E G B")), "Cmaj7");
        assert_eq!(symbol(&keys("D F A C")), "Dm7");
        assert_eq!(symbol(&keys("B D F A")), "Bm7b5");
        assert_eq!(symbol(&keys("G B F")), "G7");
        assert_eq!(symbol(&keys("C E G Bb D")), "C9");
        assert_eq!(symbol(&keys("C D E G")), "Cadd9");
    }

    #[test]
    fn the_root_in_the_bass_settles_the_same_notes() {
        assert_eq!(symbol(&keys("C E G A")), "C6");
        assert_eq!(symbol(&keys("A C E G")), "Am7");
        assert_eq!(symbol(&keys("C D G")), "Csus2");
        assert_eq!(symbol(&keys("G C D")), "Gsus4");
        assert_eq!(symbol(&keys("C Eb Gb A")), "Cdim7");
        assert_eq!(symbol(&keys("Eb Gb A C")), "Ebdim7");
    }

    #[test]
    fn keys_that_make_no_chord() {
        assert_eq!(read(&[]), None);
        let notes = |k: &[u8]| match read(k) {
            Some(Reading::Notes { notes, interval }) => (notes, interval),
            other => panic!("{other:?}"),
        };
        assert_eq!(notes(&[60]), ("C".to_owned(), None));
        assert_eq!(notes(&[60, 72]), ("C".to_owned(), Some("octave")));
        assert_eq!(notes(&[60, 64]), ("C E".to_owned(), Some("major third")));
        assert_eq!(notes(&[61, 63]), ("Db Eb".to_owned(), Some("major second")));
        assert_eq!(notes(&[60, 61, 62]), ("C Db D".to_owned(), None));
    }

    use std::collections::BTreeSet;

    type Fingers = Vec<u8>;

    /// What each source gives for three-note triads, lowest key first, the same in every key:
    /// (source, for minor triads too, [right hand, left hand] per inversion; `None`: not given).
    #[expect(clippy::type_complexity, reason = "a table")]
    const TRIADS: &[(&str, bool, [[Option<[u8; 3]>; 2]; 3])] = &[
        // Measured from the score, every key: the left hand's first inversion differs for minor.
        (
            "Salvation Army, major",
            false,
            [
                [Some([1, 3, 5]), Some([5, 3, 1])],
                [Some([1, 2, 5]), Some([5, 3, 1])],
                [Some([1, 2, 4]), Some([5, 2, 1])],
            ],
        ),
        // Hoffman's text: "all fingerings here use the third finger except first inversion on
        // the right hand and second inversion on the left, which use finger 2"; its chart, C major.
        (
            "Hoffman Academy",
            true,
            [
                [Some([1, 3, 5]), Some([5, 3, 1])],
                [Some([1, 2, 5]), Some([5, 3, 1])],
                [Some([1, 3, 5]), Some([5, 2, 1])],
            ],
        ),
        // Lauren's chart: RH 135 125 135, LH 531 531 521.
        (
            "Piano with Lauren",
            true,
            [
                [Some([1, 3, 5]), Some([5, 3, 1])],
                [Some([1, 2, 5]), Some([5, 3, 1])],
                [Some([1, 3, 5]), Some([5, 2, 1])],
            ],
        ),
        // Yamaha: thumb, middle finger and pinky, major and minor, root position.
        ("Yamaha", true, [[Some([1, 3, 5]), None], [None, None], [None, None]]),
    ];
    const SA_MINOR: [[Option<[u8; 3]>; 2]; 3] = [
        [Some([1, 3, 5]), Some([5, 3, 1])],
        [Some([1, 2, 5]), Some([4, 2, 1])],
        [Some([1, 2, 4]), Some([5, 2, 1])],
    ];

    /// Four notes, the bottom one doubled: the Salvation Army score, the same in all 24 keys.
    const SA_FOUR: [[[u8; 4]; 2]; 3] = [
        [[1, 2, 3, 5], [5, 3, 2, 1]],
        [[1, 2, 4, 5], [5, 4, 2, 1]],
        [[1, 2, 3, 5], [5, 3, 2, 1]],
    ];

    /// musicandhealth.co.uk's broken chords, first choices, verbatim ("RH 1235 1245 1245 1235";
    /// the fourth cell, root position again, left out): (roots, minor, right hand, left hand).
    #[expect(clippy::type_complexity, reason = "a table")]
    const MH_FOUR: &[(&[&str], bool, [&str; 3], [&str; 3])] = &[
        // example.htm (C major, from its image).
        (&["C"], false, ["1235", "1245", "1245"], ["5421", "5421", "5321"]),
        // brokenchord1.htm
        (
            &["F", "C", "G"],
            true,
            ["1235", "1245", "1235"],
            ["5421", "5421", "5321"],
        ),
        // brokenchord2.htm
        (
            &["D", "A", "E"],
            false,
            ["1235", "1245", "1245"],
            ["5321", "5421", "5321"],
        ),
        // brokenchord3.htm: RH 1st inversion "1245 or 1235", LH root "5321 or 5421".
        (
            &["Eb", "Ab", "Db"],
            false,
            ["1235", "1245", "1245"],
            ["5321", "5421", "5321"],
        ),
        // brokenchord3.htm: LH 1st inversion "5321 or 5421".
        (
            &["F#", "C#", "G#"],
            true,
            ["1235", "1245", "1235"],
            ["5421", "5321", "5321"],
        ),
        // brokenchord4.htm
        (&["B", "Bb"], false, ["1235", "1245", "1245"], ["5321", "5421", "5321"]),
        (&["B", "Bb"], true, ["1235", "1245", "1235"], ["5421", "5421", "5321"]),
        // brokenchord5.htm: Eb minor LH 1st inversion "5421 or 5321".
        (&["F#"], false, ["1235", "1245", "1245"], ["5321", "5421", "5321"]),
        (&["Eb"], true, ["1235", "1245", "1235"], ["5421", "5421", "5321"]),
    ];

    fn digits(s: &str) -> Fingers {
        s.bytes().map(|b| b - b'0').collect()
    }

    fn pc(name: &str) -> u8 {
        keys(name)[0] % 12
    }

    /// Shown only when at least two sources give it and none gives another.
    fn agreed(given: &[Fingers]) -> Option<Fingers> {
        let distinct: BTreeSet<&Fingers> = given.iter().collect();
        (given.len() >= 2 && distinct.len() == 1).then(|| given[0].clone())
    }

    fn hand_index(hand: Hand) -> usize {
        usize::from(hand == Hand::Left)
    }

    fn shape(minor: bool, root: u8, inversion: u8, doubled: bool) -> Shape {
        Shape {
            minor,
            root,
            inversion,
            doubled,
        }
    }

    #[test]
    fn triads_are_fingered_only_where_the_sources_agree() {
        for minor in [false, true] {
            for inv in 0..3u8 {
                for hand in [Hand::Right, Hand::Left] {
                    let h = hand_index(hand);
                    let i = usize::from(inv);
                    let mut given: Vec<Fingers> = TRIADS
                        .iter()
                        .filter(|(name, both, _)| *both || (!minor && name.starts_with("Salvation")))
                        .filter_map(|(_, _, f)| f[i][h].map(|f| f.to_vec()))
                        .collect();
                    if minor {
                        given.extend(SA_MINOR[i][h].map(|f| f.to_vec()));
                    }
                    for root in 0..12 {
                        let got = shape(minor, root, inv, false).fingers(hand).map(<[u8]>::to_vec);
                        assert_eq!(
                            got,
                            agreed(&given),
                            "minor {minor}, inversion {inv}, {hand:?}: {given:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn four_note_chords_are_fingered_only_where_the_sources_agree() {
        for minor in [false, true] {
            for inv in 0..3u8 {
                for hand in [Hand::Right, Hand::Left] {
                    let (h, i) = (hand_index(hand), usize::from(inv));
                    let mh = |root: u8| -> Option<Fingers> {
                        MH_FOUR
                            .iter()
                            .find(|(roots, m, _, _)| *m == minor && roots.iter().any(|r| pc(r) == root))
                            .map(|(_, _, rh, lh)| digits(if hand == Hand::Right { rh[i] } else { lh[i] }))
                    };
                    // A key it does not cover: its fingering where it is the same in every key.
                    let covered: BTreeSet<Fingers> = (0..12).filter_map(mh).collect();
                    for root in 0..12 {
                        let mut given = vec![SA_FOUR[i][h].to_vec()];
                        given.extend(
                            mh(root).or_else(|| (covered.len() == 1).then(|| covered.first().unwrap().clone())),
                        );
                        let got = shape(minor, root, inv, true).fingers(hand).map(<[u8]>::to_vec);
                        assert_eq!(
                            got,
                            agreed(&given),
                            "minor {minor}, root {root}, inversion {inv}, {hand:?}"
                        );
                    }
                }
            }
        }
    }

    /// (hand, finger) per key, lowest key first.
    fn on_keys(keys: &[u8]) -> Vec<(u8, Hand, u8)> {
        let mut f = fingering(keys);
        f.sort_by_key(|&(k, h, _)| (k, hand_index(h)));
        f
    }

    #[test]
    fn a_chord_in_one_hand_shows_each_hand_that_is_agreed() {
        // C major root position: both hands.
        assert_eq!(
            on_keys(&[60, 64, 67]),
            [
                (60, Hand::Right, 1),
                (60, Hand::Left, 5),
                (64, Hand::Right, 3),
                (64, Hand::Left, 3),
                (67, Hand::Right, 5),
                (67, Hand::Left, 1)
            ]
        );
        // Second inversion: the left hand only.
        assert_eq!(
            on_keys(&[67, 72, 76]),
            [(67, Hand::Left, 5), (72, Hand::Left, 2), (76, Hand::Left, 1)]
        );
        // A minor, first inversion: the right hand only.
        assert_eq!(
            on_keys(&[60, 64, 69]),
            [(60, Hand::Right, 1), (64, Hand::Right, 2), (69, Hand::Right, 5)]
        );
        // Not a shape in the table: nothing.
        assert_eq!(fingering(&keys("B D F")), []);
        assert_eq!(fingering(&keys("G B D F")), []);
        assert_eq!(fingering(&keys("C D E")), []);
        assert_eq!(fingering(&[60, 64]), []);
    }

    #[test]
    fn wide_chords_are_split_between_the_hands() {
        // A bass note (no fingering) and E G C in the right hand.
        assert_eq!(
            on_keys(&[48, 64, 67, 72]),
            [(64, Hand::Right, 1), (67, Hand::Right, 2), (72, Hand::Right, 5)]
        );
        // D major in each hand, root position: left 5 3 2 1 on D F# A D, right 1 3 5.
        assert_eq!(
            on_keys(&[38, 42, 45, 50, 62, 66, 69]),
            [
                (38, Hand::Left, 5),
                (42, Hand::Left, 3),
                (45, Hand::Left, 2),
                (50, Hand::Left, 1),
                (62, Hand::Right, 1),
                (66, Hand::Right, 3),
                (69, Hand::Right, 5)
            ]
        );
        // Out of reach of two hands: nothing.
        assert_eq!(fingering(&[36, 50, 64, 80]), []);
    }
}
