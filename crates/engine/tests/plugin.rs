//! A real VST3 instrument keeps the instrument contract (`common`), and its sound (state)
//! survives a save and restore. Rendered offline (no audio device or keyboard).
//!
//! The plugin is `$ONEZAN_TEST_PLUGIN` (a `.vst3` path), else the first installed instrument.
//! Without one, each test prints a note and passes: these are for machines that have a plugin.
//!
//! The tests run one at a time ([`one_at_a_time`]), as the app uses plugins: some cannot have
//! instances on several threads at once (Dexed, a JUCE plugin, hung that way).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test helpers: a failure is a test failure"
)]

mod common;

use common::{RATE, SEC, on, peak};
use onezan_engine::{Instrument, render_offline};
use onezan_vst3_host::Plugin;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Held for the whole of a test, taken before its plugin is loaded (so it is dropped after).
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn plugin() -> Option<Plugin> {
    let bundle = std::env::var_os("ONEZAN_TEST_PLUGIN").map(PathBuf::from).or_else(|| {
        let bundles = onezan_vst3_host::find_bundles(&onezan_vst3_host::default_search_paths());
        onezan_vst3_host::scan(&bundles)
            .into_iter()
            .find(onezan_vst3_host::PluginInfo::is_instrument)
            .map(|p| p.bundle)
    });
    let Some(bundle) = bundle else {
        eprintln!("no VST3 instrument installed; skipping");
        return None;
    };
    Some(Plugin::load(&bundle, None).expect("plugin loads"))
}

fn instrument() -> Option<Instrument> {
    plugin().map(Instrument::Plugin)
}

instrument_contract!(instrument, one_at_a_time());

#[test]
fn state_restores_into_a_fresh_instance() {
    let _one = one_at_a_time();
    let Some(a) = plugin() else { return };
    let saved = a.state().unwrap();
    assert!(!saved.component.is_empty(), "the plugin wrote no state");
    drop(a);
    // Surge XT keeps its patch as XML: rename it (same length, so its own size fields stay
    // right) to tell a restored state from a fresh one.
    let mut changed = saved;
    let renamed = replace(&mut changed.component, b"name=\"Init Saw\"", b"name=\"Init Sax\"");
    let Some(mut b) = plugin() else { return };
    let state = onezan_vst3_host::State::from_bytes(&changed.to_bytes()).unwrap();
    b.set_state(&state).unwrap();
    let after = b.state().unwrap();
    if renamed {
        assert!(
            contains(&after.component, b"name=\"Init Sax\""),
            "the restored patch name is lost"
        );
    } else {
        eprintln!("not Surge XT's default patch: checked only that the state is taken back");
        assert_eq!(after.component, changed.component);
    }
    // And it still plays.
    let (l, r) = render_offline(&mut Instrument::Plugin(b), RATE, SEC / 2, vec![(0, on(60))]).unwrap();
    assert!(peak(&l, &r) > common::AUDIBLE, "silent after restoring");
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Replaces the first `from` in `bytes` with `to` (same length); whether it was there.
fn replace(bytes: &mut [u8], from: &[u8], to: &[u8]) -> bool {
    let Some(i) = bytes.windows(from.len()).position(|w| w == from) else {
        return false;
    };
    bytes[i..][..to.len()].copy_from_slice(to);
    true
}
