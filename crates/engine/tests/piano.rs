//! The built-in piano keeps the instrument contract (`common`). Needs the SoundFont
//! `build/fetch-piano.sh` downloads; without it, each test prints a note and passes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test helpers: a failure is a test failure"
)]

mod common;

use onezan_engine::{Instrument, Piano};
use std::path::Path;

fn piano() -> Option<Instrument> {
    let sf2 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/piano/UprightPianoKW-20220221.sf2");
    let Ok(bytes) = std::fs::read(&sf2) else {
        eprintln!("{} missing (build/fetch-piano.sh); skipping", sf2.display());
        return None;
    };
    Some(Instrument::Piano(Piano::from_sf2("piano", &bytes).unwrap()))
}

instrument_contract!(piano);

#[test]
fn a_file_that_is_not_a_soundfont_is_refused() {
    assert!(Piano::from_sf2("x", b"RIFF\0\0\0\0not a soundfont").is_err());
    assert!(Piano::from_sf2("x", b"").is_err());
}
