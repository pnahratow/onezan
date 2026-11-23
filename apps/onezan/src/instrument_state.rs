//! The instrument's sound between starts: the plugin's own state, saved on quit and restored
//! after loading, one file per plugin (`<data dir>/instruments/<class id>.state`).
//!
//! Best effort, never in the way of starting: a missing file means the plugin's default sound; a
//! damaged one, or one the plugin refuses, is moved aside and the default sound is used. Restoring
//! runs inside our process, so a plugin that crashes on its saved state would crash every start:
//! a marker file is written before restoring and removed after, and a marker left behind by a
//! crash makes the next start skip that state (moved aside as `.crashed`).

use crate::settings::data_dir;
use onezan_vst3_host::{Plugin, State};
use std::path::{Path, PathBuf};
use std::{fs, io};

fn state_path(dir: &Path, plugin: &Plugin) -> PathBuf {
    dir.join(format!("{}.state", plugin.class_id()))
}

fn instruments_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("instruments"))
}

/// Restores the sound saved for `plugin`. Returns what happened, for the startup report
/// (empty: nothing was saved).
pub fn restore(plugin: &mut Plugin) -> String {
    instruments_dir().map_or_else(String::new, |dir| restore_from(&dir, plugin))
}

fn restore_from(dir: &Path, plugin: &mut Plugin) -> String {
    let path = state_path(dir, plugin);
    let marker = path.with_extension("restoring");
    if marker.exists() {
        let _ = fs::remove_file(&marker);
        set_aside(&path, "crashed");
        return "default sound: restoring the saved one crashed last time (moved aside)".into();
    }
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return String::new(),
        Err(e) => return format!("default sound: cannot read the saved one ({e})"),
    };
    let Some(state) = State::from_bytes(&bytes) else {
        set_aside(&path, "broken");
        return "default sound: the saved one is damaged (moved aside)".into();
    };
    // If the plugin crashes in here, the marker stays and the next start skips this state.
    let _ = fs::write(&marker, b"");
    let result = plugin.set_state(&state);
    let _ = fs::remove_file(&marker);
    match result {
        Ok(()) => "sound from last time".into(),
        Err(e) => {
            set_aside(&path, "refused");
            log::warn!("{e}");
            "default sound: the plugin refused the saved one (moved aside)".into()
        }
    }
}

/// Saves `plugin`'s sound for the next start. Problems are logged, never fatal.
pub fn save(plugin: &Plugin) {
    if let Some(dir) = instruments_dir() {
        save_to(&dir, plugin);
    }
}

fn save_to(dir: &Path, plugin: &Plugin) {
    let state = match plugin.state() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("the instrument's sound is not saved: {e}");
            return;
        }
    };
    let path = state_path(dir, plugin);
    let tmp = path.with_extension("state.tmp");
    let written = fs::create_dir_all(dir)
        .and_then(|()| fs::write(&tmp, state.to_bytes()))
        .and_then(|()| fs::rename(&tmp, &path));
    if let Err(e) = written {
        log::warn!("cannot save the instrument's sound to {}: {e}", path.display());
    }
}

/// Moves `path` to `<path>.<why>` so it is kept for a look but not used again.
fn set_aside(path: &Path, why: &str) {
    let mut aside = path.as_os_str().to_owned();
    aside.push(format!(".{why}"));
    if let Err(e) = fs::rename(path, &aside)
        && e.kind() != io::ErrorKind::NotFound
    {
        log::warn!("cannot move {} aside: {e}", path.display());
    }
}
