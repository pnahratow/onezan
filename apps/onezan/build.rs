//! Embeds the built-in piano (a SoundFont) when `build/fetch-piano.sh` has put it in
//! `assets/piano/`; without it the program builds, but has no built-in piano.

use std::path::Path;

fn main() {
    let sf2 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/piano/UprightPianoKW-20220221.sf2");
    println!("cargo::rustc-check-cfg=cfg(onezan_piano)");
    // The file, and its folder: a file put back keeps its old time, the folder's changes.
    println!("cargo::rerun-if-changed={}", sf2.display());
    if let Some(dir) = sf2.parent() {
        println!("cargo::rerun-if-changed={}", dir.display());
    }
    if sf2.is_file() {
        println!("cargo::rustc-cfg=onezan_piano");
        println!("cargo::rustc-env=ONEZAN_PIANO_SF2={}", sf2.display());
    } else {
        println!(
            "cargo::warning=no built-in piano: {} is missing (run build/fetch-piano.sh)",
            sf2.display()
        );
    }
}
