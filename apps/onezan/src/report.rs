//! The startup report as text, for the terminal.

use crate::session::Session;
use onezan_engine::{Considered, MidiChange, Verdict};

/// Prints one section of the startup report: what is used (with `detail`), why, the
/// alternatives with the reason each was not used, and `hint` (how to choose differently).
pub fn print_section(label: &str, considered: &[Considered], detail: &str, hint: &str) {
    let used = considered.iter().find(|c| c.verdict == Verdict::Used);
    match used {
        Some(u) if detail.is_empty() => println!("{label:<10}{}", u.name),
        Some(u) => println!("{label:<10}{}, {detail}", u.name),
        None => println!("{label:<10}none"),
    }
    if let Some(u) = used {
        println!("{:<10}  why: {}", "", u.why);
    }
    for c in considered.iter().filter(|c| c.verdict != Verdict::Used) {
        let state = if c.verdict == Verdict::Unavailable {
            "unavailable"
        } else {
            "not used"
        };
        println!("{:<10}  also: {} ({state}: {})", "", c.name, c.why);
    }
    if !hint.is_empty() {
        println!("{:<10}  {hint}", "");
    }
}

/// Prints the startup report.
pub fn print_report(s: &Session) {
    for sec in &s.report {
        print_section(sec.label, &sec.considered, &sec.detail, sec.hint);
    }
    match (&s.first_midi, s.section("MIDI in").is_some()) {
        (Some(MidiChange::Connected(_)), _) | (_, false) => {}
        (Some(c), true) => println!("{:<10}  {}", "", midi_change(c)),
        (None, true) => println!("{:<10}  no keyboard yet: plug one in and it is used", ""),
    }
}

/// A MIDI port change, for the user.
pub fn midi_change(c: &MidiChange) -> String {
    match c {
        MidiChange::Connected(p) => format!("{p} connected"),
        MidiChange::Disconnected(p) => format!("{p} unplugged; held notes released; waiting for a keyboard"),
        MidiChange::Failed { port, error } => format!("cannot open {port}: {error}; retrying"),
    }
}
