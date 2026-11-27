//! The line above the keyboard as a guide for someone new: the next thing worth knowing, one at
//! a time, until it has been done. What was done is kept in settings.json (`tips_done`), so a
//! tip does not come back. No keyboard is a state, not a step: that tip shows while there is
//! none.

/// A tip, in the order they come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    Keyboard,
    Scale,
    Metronome,
    LearnSong,
}

/// Notes played on the keyboard, with a scale shown, before the scale's tip counts as done.
pub const SCALE_NOTES: u32 = 8;

impl Tip {
    /// Its name in settings.json.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Scale => "scale",
            Self::Metronome => "metronome",
            Self::LearnSong => "learn_song",
        }
    }

    /// What the line says; `scale` is the one shown ("C major").
    pub fn text(self, scale: &str) -> String {
        match self {
            Self::Keyboard => "No keyboard yet: plug one in, or click the keys below to try it".to_owned(),
            Self::Scale => format!("Play {scale}: right hand blue, left hand orange. Another scale: Scale"),
            Self::Metronome => "Space starts the metronome, T taps the tempo".to_owned(),
            Self::LearnSong => "Learn Song: a MIDI file falls onto the keys, to play along".to_owned(),
        }
    }
}

/// What the tips go by.
pub struct Facts<'a> {
    /// A MIDI keyboard is connected.
    pub keyboard: bool,
    /// A scale is shown on the keys.
    pub scale: bool,
    /// The ids of the tips done.
    pub done: &'a [String],
}

/// The tip to show; `None`: nothing left to say.
pub fn next(facts: &Facts<'_>) -> Option<Tip> {
    if !facts.keyboard {
        return Some(Tip::Keyboard);
    }
    let done = |t: Tip| facts.done.iter().any(|d| d == t.id());
    [Tip::Scale, Tip::Metronome, Tip::LearnSong]
        .into_iter()
        .find(|&t| !done(t) && (t != Tip::Scale || facts.scale))
}

/// Notes `done` (by id) if it isn't yet; whether that changed anything.
pub fn mark(done: &mut Vec<String>, tip: Tip) -> bool {
    if done.iter().any(|d| d == tip.id()) {
        return false;
    }
    done.push(tip.id().to_owned());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(keyboard: bool, scale: bool, done: &[&str]) -> Option<Tip> {
        let done: Vec<String> = done.iter().map(|&d| d.to_owned()).collect();
        next(&Facts {
            keyboard,
            scale,
            done: &done,
        })
    }

    #[test]
    fn the_tips_come_one_at_a_time_until_each_is_done() {
        assert_eq!(tip(true, true, &[]), Some(Tip::Scale));
        assert_eq!(tip(true, true, &["scale"]), Some(Tip::Metronome));
        assert_eq!(tip(true, true, &["scale", "metronome"]), Some(Tip::LearnSong));
        assert_eq!(tip(true, true, &["scale", "metronome", "learn_song"]), None);
        // Done out of order: the next one not done.
        assert_eq!(tip(true, true, &["metronome"]), Some(Tip::Scale));
        assert_eq!(tip(true, true, &["learn_song", "scale"]), Some(Tip::Metronome));
    }

    #[test]
    fn without_a_keyboard_that_comes_first_whatever_was_done() {
        assert_eq!(tip(false, true, &[]), Some(Tip::Keyboard));
        assert_eq!(
            tip(false, false, &["scale", "metronome", "learn_song"]),
            Some(Tip::Keyboard)
        );
    }

    #[test]
    fn without_a_scale_shown_its_tip_is_passed_over() {
        assert_eq!(tip(true, false, &[]), Some(Tip::Metronome));
    }

    #[test]
    fn marking_a_tip_done_twice_keeps_one() {
        let mut done = Vec::new();
        assert!(mark(&mut done, Tip::Metronome));
        assert!(!mark(&mut done, Tip::Metronome));
        assert_eq!(done, ["metronome"]);
    }

    #[test]
    fn the_scale_tip_names_the_scale() {
        assert!(Tip::Scale.text("D major").starts_with("Play D major:"));
    }
}
