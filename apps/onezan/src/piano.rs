//! The built-in piano: FreePats' "Upright Piano KW" (CC0), embedded in the program by
//! `build.rs` when `build/fetch-piano.sh` has fetched it. It plays when no VST3 instrument is
//! installed or none loads, and can be chosen like any instrument.

use anyhow::{Result, bail};
use onezan_engine::Piano;
use std::sync::OnceLock;

/// What the window and the report call it.
pub const NAME: &str = "Built-in piano";

#[cfg(onezan_piano)]
const SF2: Option<&[u8]> = Some(include_bytes!(env!("ONEZAN_PIANO_SF2")));
#[cfg(not(onezan_piano))]
const SF2: Option<&[u8]> = None;

/// Whether this build has the built-in piano.
pub const fn available() -> bool {
    SF2.is_some()
}

/// The built-in piano, parsed the first time it is needed.
///
/// # Errors
///
/// When this build has none (built without `build/fetch-piano.sh`), or it cannot be read.
pub fn load() -> Result<Piano> {
    static PIANO: OnceLock<Result<Piano, String>> = OnceLock::new();
    let Some(sf2) = SF2 else {
        bail!("this build has no built-in piano (build/fetch-piano.sh was not run)");
    };
    match PIANO.get_or_init(|| Piano::from_sf2(NAME, sf2).map_err(|e| format!("{e:#}"))) {
        Ok(p) => Ok(p.clone()),
        Err(e) => bail!("the built-in piano: {e}"),
    }
}
