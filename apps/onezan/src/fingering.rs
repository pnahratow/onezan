//! Scale fingerings, for display on the keyboard.
//!
//! For now one hand-written example, C major over two octaves, to show the keyboard's fingering
//! badges. The scale trainer's full table (every scale, both hands) replaces this.

/// Which fingering is shown on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preview {
    #[default]
    Off,
    /// C major, two octaves, hands together: left hand C2 to C4, right hand C4 to C6.
    CMajor,
}

impl Preview {
    pub const ALL: [Self; 2] = [Self::Off, Self::CMajor];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "none",
            Self::CMajor => "C major, both hands",
        }
    }

    /// The finger of each hand on `key`: (left, right).
    pub fn fingers(self, key: u8) -> (Option<u8>, Option<u8>) {
        match self {
            Self::Off => (None, None),
            Self::CMajor => (finger(&C_MAJOR_LEFT, 36, key), finger(&C_MAJOR_RIGHT, 60, key)),
        }
    }

    /// Whether `key` belongs to the scale shown.
    pub const fn in_scale(self, key: u8) -> bool {
        match self {
            Self::Off => false,
            Self::CMajor => matches!(key % 12, 0 | 2 | 4 | 5 | 7 | 9 | 11),
        }
    }
}

/// The major scale's steps over two octaves, in semitones above the root (15 notes).
const MAJOR_TWO_OCTAVES: [u8; 15] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24];

/// C major ascending, two octaves (the same fingers serve descending).
const C_MAJOR_RIGHT: [u8; 15] = [1, 2, 3, 1, 2, 3, 4, 1, 2, 3, 1, 2, 3, 4, 5];
const C_MAJOR_LEFT: [u8; 15] = [5, 4, 3, 2, 1, 3, 2, 1, 4, 3, 2, 1, 3, 2, 1];

/// The finger for `key` in a two-octave scale from `root` played with `fingers`.
fn finger(fingers: &[u8; 15], root: u8, key: u8) -> Option<u8> {
    let step = key.checked_sub(root)?;
    let i = MAJOR_TWO_OCTAVES.iter().position(|&s| s == step)?;
    fingers.get(i).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_major_hands_meet_on_middle_c() {
        let p = Preview::CMajor;
        assert_eq!(p.fingers(60), (Some(1), Some(1)));
        assert_eq!(p.fingers(36), (Some(5), None));
        assert_eq!(p.fingers(84), (None, Some(5)));
        // F above middle C: right thumb after the turn.
        assert_eq!(p.fingers(65), (None, Some(1)));
        // A black key is not in C major.
        assert_eq!(p.fingers(61), (None, None));
        assert!(!p.in_scale(61));
    }
}
