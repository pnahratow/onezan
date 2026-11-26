//! Settings kept between starts: a small JSON file in the platform's config folder
//! (`~/.config/onezan/settings.json` on Linux, `%APPDATA%\onezan\settings.json` on Windows).
//!
//! Every value is optional: what is missing keeps the built-in default, unknown keys are ignored,
//! and a value that no longer fits (a scale or sound that is gone) falls back to the default too.
//! A file that cannot be read is moved aside (`settings.json.broken`) rather than overwritten,
//! so nothing the user wrote by hand is lost.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches, as the file keeps them"
)]
pub struct Settings {
    pub metronome: MetronomeSettings,
    /// The fingering shown, by name ("D major"); missing: none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<String>,
    /// The window's inner size in points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<[f32; 2]>,
    /// The instrument used last time (its plugin class id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,
    /// The MIDI keyboard used last time (its port name, without the ALSA address).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi: Option<String>,
    /// The audio output chosen by hand; missing: the automatic choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioSetting>,
    /// The MIDI timing ("auto", "exact", "immediate"); missing: auto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_timing: Option<String>,
    /// The instruments starred in the instruments window (plugin class ids), listed first there.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub favorite_instruments: Vec<String>,
    /// The scales starred in the scales window (by name, "D major"), listed first there.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub favorite_scales: Vec<String>,
    /// The falling notes are shown in place of the instrument's own window.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub notes_view: bool,
    /// The falling notes wait at the keys until played.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
    /// The app plays the falling notes through the instrument.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub play_notes: bool,
    /// Playing along without hearing the metronome.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub notes_silent: bool,
    /// "light"; missing: dark.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// The built-in piano's volume (1.0: its own level); missing: 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piano_volume: Option<f32>,
    /// The song opened last (a MIDI file); missing: the scale falls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<PathBuf>,
}

/// An audio output: the audio system ("ALSA", "JACK", "WASAPI") and the device's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSetting {
    pub backend: String,
    pub device: String,
}

impl From<AudioSetting> for onezan_engine::Output {
    fn from(a: AudioSetting) -> Self {
        Self {
            backend: a.backend,
            device: a.device,
        }
    }
}

impl From<onezan_engine::Output> for AudioSetting {
    fn from(o: onezan_engine::Output) -> Self {
        Self {
            backend: o.backend,
            device: o.device,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetronomeSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<u32>,
    /// 0.0 to 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    /// The sound's name, as the options window shows it ("Woodblock").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
}

/// The folder for settings: `$XDG_CONFIG_HOME/onezan` or `~/.config/onezan` on Linux,
/// `%APPDATA%\onezan` on Windows. `None` when the system names no home folder.
pub fn config_dir() -> Option<PathBuf> {
    platform_dir("XDG_CONFIG_HOME", ".config")
}

/// The folder for data (the practice history): `$XDG_DATA_HOME/onezan` or
/// `~/.local/share/onezan` on Linux, `%APPDATA%\onezan` on Windows.
pub fn data_dir() -> Option<PathBuf> {
    platform_dir("XDG_DATA_HOME", ".local/share")
}

fn platform_dir(xdg: &str, home_relative: &str) -> Option<PathBuf> {
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").and_then(absolute)
    } else {
        std::env::var_os(xdg).and_then(absolute).or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute)
                .map(|h| h.join(home_relative))
        })
    };
    base.map(|b| b.join("onezan"))
}

/// The settings file, and what was last written to it.
pub struct Store {
    path: Option<PathBuf>,
    saved: Settings,
}

impl Store {
    /// Reads the settings from the usual place. Never fails: problems are logged, and the
    /// defaults are used.
    pub fn open() -> (Self, Settings) {
        config_dir().map_or_else(
            || {
                log::warn!("no home folder: settings will not be saved");
                (
                    Self {
                        path: None,
                        saved: Settings::default(),
                    },
                    Settings::default(),
                )
            },
            |dir| Self::load(&dir.join("settings.json")),
        )
    }

    /// Reads the settings at `path` (missing: the defaults).
    pub fn load(path: &Path) -> (Self, Settings) {
        let settings = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                let aside = path.with_extension("json.broken");
                log::warn!("cannot read {}: {e}; moved to {}", path.display(), aside.display());
                if let Err(e) = fs::rename(path, &aside) {
                    log::warn!("cannot move it: {e}");
                }
                Settings::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                log::warn!("cannot read {}: {e}", path.display());
                Settings::default()
            }
        };
        (
            Self {
                path: Some(path.to_owned()),
                saved: settings.clone(),
            },
            settings,
        )
    }

    /// Whether `settings` differ from what was last read or written.
    pub fn changed(&self, settings: &Settings) -> bool {
        *settings != self.saved
    }

    /// Writes `settings` if they changed. Written to a temporary file first and then renamed
    /// over the old one, so a crash mid-write leaves the old file whole.
    pub fn save(&mut self, settings: &Settings) {
        if !self.changed(settings) {
            return;
        }
        let Some(path) = &self.path else { return };
        match write(path, settings) {
            Ok(()) => self.saved = settings.clone(),
            Err(e) => {
                log::warn!("cannot save the settings to {}: {e}", path.display());
                // Not retried every frame: the next change tries again.
                self.saved = settings.clone();
            }
        }
    }
}

fn write(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onezan-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("settings.json")
    }

    fn sample() -> Settings {
        Settings {
            metronome: MetronomeSettings {
                bpm: Some(90),
                volume: Some(0.5),
                sound: Some("Woodblock".into()),
            },
            scale: Some("D major".into()),
            window: Some([1200.0, 760.0]),
            instrument: Some("ABCD".into()),
            midi: Some("Keystation".into()),
            audio: Some(AudioSetting {
                backend: "ALSA".into(),
                device: "default".into(),
            }),
            midi_timing: Some("exact".into()),
            favorite_instruments: vec!["ABCD".into(), "built-in-piano".into()],
            favorite_scales: vec!["D major".into()],
            notes_view: true,
            wait: true,
            play_notes: true,
            notes_silent: true,
            theme: Some("light".into()),
            piano_volume: Some(0.7),
            song: Some(PathBuf::from("/songs/Für Elise.mid")),
        }
    }

    #[test]
    fn a_settings_file_from_version_0_1_still_loads() {
        let path = temp("fixture");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, include_str!("../fixtures/settings-0.1.json")).unwrap();
        let (_, s) = Store::load(&path);
        let expected = Settings {
            instrument: Some("built-in-piano".into()),
            midi: Some("SWISSONIC EasyKeys49:SWISSONIC EasyKeys49 MIDI 1".into()),
            audio: Some(AudioSetting {
                backend: "JACK".into(),
                device: "default".into(),
            }),
            favorite_instruments: Vec::new(),
            favorite_scales: Vec::new(),
            notes_view: false,
            wait: false,
            play_notes: false,
            notes_silent: false,
            theme: None,
            piano_volume: None,
            song: None,
            ..sample()
        };
        assert_eq!(s, expected);
        assert!(path.exists(), "the file was moved aside");
    }

    #[test]
    fn folders_follow_the_platform() {
        let (config, data) = (config_dir().unwrap(), data_dir().unwrap());
        assert!(config.ends_with("onezan") && data.ends_with("onezan"));
        if cfg!(windows) {
            let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap());
            assert_eq!(config, appdata.join("onezan"));
            assert_eq!(data, appdata.join("onezan"));
        } else if std::env::var_os("XDG_CONFIG_HOME").is_none() {
            assert!(config.ends_with(".config/onezan"), "{}", config.display());
        }
    }

    #[test]
    fn saves_and_reads_back() {
        let path = temp("roundtrip");
        let (mut store, first) = Store::load(&path);
        assert_eq!(first, Settings::default());
        store.save(&sample());
        let (_, again) = Store::load(&path);
        assert_eq!(again, sample());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn missing_and_unknown_keys_are_fine() {
        let path = temp("partial");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{ "metronome": { "bpm": 72 }, "later": true }"#).unwrap();
        let (_, s) = Store::load(&path);
        assert_eq!(s.metronome.bpm, Some(72));
        assert_eq!(s.metronome.sound, None);
        assert_eq!(s.scale, None);
    }

    #[test]
    fn a_broken_file_is_moved_aside_not_overwritten() {
        let path = temp("broken");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        let (mut store, s) = Store::load(&path);
        assert_eq!(s, Settings::default());
        assert_eq!(
            fs::read_to_string(path.with_extension("json.broken")).unwrap(),
            "{ not json"
        );
        store.save(&sample());
        assert_eq!(Store::load(&path).1, sample());
    }

    #[test]
    fn unchanged_settings_are_not_written() {
        let path = temp("unchanged");
        let (mut store, s) = Store::load(&path);
        store.save(&s);
        assert!(!path.exists());
    }

    #[test]
    fn the_file_is_readable_json() {
        let text = serde_json::to_string_pretty(&sample()).unwrap();
        assert!(text.contains(r#""bpm": 90"#));
        assert!(text.contains(r#""scale": "D major""#));
        // Nothing is written for what was never set.
        assert_eq!(
            serde_json::to_string(&Settings::default()).unwrap(),
            r#"{"metronome":{}}"#
        );
    }
}
