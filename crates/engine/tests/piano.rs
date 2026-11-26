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

#[test]
fn the_volume_scales_the_piano() {
    use common::{on, peak};
    use onezan_engine::{MAX_PIANO_VOLUME, render_offline};
    let Some(Instrument::Piano(p)) = piano() else { return };
    let loudness = |volume: f32| {
        p.set_volume(volume);
        let mut i = Instrument::Piano(p.clone());
        let (l, r) = render_offline(&mut i, 48_000, 24_000, vec![(0, on(60))]).unwrap();
        peak(&l, &r)
    };
    let full = loudness(1.0);
    assert!(full > 0.01, "sounds at its own level");
    assert!(
        (loudness(0.5) / full - 0.5).abs() < 0.02,
        "half the level at half volume"
    );
    assert!(loudness(0.0) < 1e-6, "silent at 0");
    p.set_volume(10.0);
    assert!((p.volume() - MAX_PIANO_VOLUME).abs() < f32::EPSILON, "clamped");
}
