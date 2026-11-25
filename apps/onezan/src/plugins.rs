//! Which instrument to load: the installed plugins, the built-in piano, and the one an argument
//! (or nothing) picks, with the verdict on each for the startup report.

use anyhow::{Context, Result, bail};
use onezan_engine::{BUILT_IN_PIANO, Considered, Instrument, Verdict};
use onezan_vst3_host::{Plugin, PluginInfo};
use std::path::{Path, PathBuf};

/// The installed VST3 plugins.
pub fn installed() -> Vec<PluginInfo> {
    onezan_vst3_host::scan(&onezan_vst3_host::find_bundles(
        &onezan_vst3_host::default_search_paths(),
    ))
}

/// Resolves the plugin argument to (bundle, class id), with the verdict on each instrument.
pub fn resolve(arg: Option<&str>) -> Result<(PathBuf, Option<String>, Vec<Considered>)> {
    if let Some(a) = arg {
        let p = Path::new(a);
        if p.exists() {
            let c = Considered {
                name: a.to_string(),
                verdict: Verdict::Used,
                why: "the path given".into(),
            };
            return Ok((p.to_path_buf(), None, vec![c]));
        }
    }
    let plugins = installed();
    let pick = arg.map_or_else(
        || plugins.iter().position(PluginInfo::is_instrument),
        |a| {
            let q = a.to_lowercase();
            plugins
                .iter()
                .position(|p| p.class_id.eq_ignore_ascii_case(a))
                .or_else(|| {
                    plugins
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.name.to_lowercase().contains(&q))
                        .max_by_key(|(_, p)| (p.is_instrument(), p.name.len() == a.len()))
                        .map(|(i, _)| i)
                })
        },
    );
    let considered = plugins
        .iter()
        .enumerate()
        .filter(|&(i, p)| p.is_instrument() || Some(i) == pick)
        .map(|(i, p)| {
            let (verdict, why) = match (Some(i) == pick, arg) {
                (true, Some(a)) => (Verdict::Used, format!("matches \"{a}\"")),
                (true, None) => (Verdict::Used, "the first installed instrument".to_string()),
                (false, Some(a)) => (Verdict::NotUsed, format!("does not match \"{a}\"")),
                (false, None) => (Verdict::NotUsed, "another instrument was picked first".to_string()),
            };
            Considered {
                name: format!("{} ({})", p.name, p.vendor),
                verdict,
                why,
            }
        })
        .collect();
    match pick.and_then(|i| plugins.get(i)) {
        Some(p) => Ok((p.bundle.clone(), Some(p.class_id.clone()), considered)),
        None if arg.is_none() => bail!("no VST3 instrument installed (try `onezan list --all`)"),
        None => bail!(
            "no installed plugin matches {:?} (see `onezan list`)",
            arg.unwrap_or_default()
        ),
    }
}

/// Where instruments come from, for [`choose`]. [`Installed`] loads the real ones; the tests
/// use a fake, so the rules of choosing are checked without plugins.
pub trait Source {
    type Instrument;
    /// The plugin `arg` names (a path, a class id or part of a name); without one, the first
    /// installed instrument. With the verdict on each installed one.
    fn plugin(&mut self, arg: Option<&str>) -> Result<(Self::Instrument, Vec<Considered>)>;
    /// The built-in piano; an error when this build has none.
    fn piano(&mut self) -> Result<Self::Instrument>;
    /// Whether this build has the built-in piano (without loading it).
    fn has_piano(&self) -> bool;
}

/// The installed plugins and this build's built-in piano.
pub struct Installed;

impl Source for Installed {
    type Instrument = Instrument;

    fn plugin(&mut self, arg: Option<&str>) -> Result<(Instrument, Vec<Considered>)> {
        let (bundle, class, considered) = resolve(arg)?;
        let p = Plugin::load(&bundle, class.as_deref()).with_context(|| format!("loading {}", bundle.display()))?;
        Ok((Instrument::Plugin(p), considered))
    }

    fn piano(&mut self) -> Result<Instrument> {
        crate::piano::load().map(Instrument::Piano)
    }

    fn has_piano(&self) -> bool {
        crate::piano::available()
    }
}

/// The instrument chosen, with the verdict on each one considered.
pub struct Chosen<I> {
    pub instrument: I,
    pub considered: Vec<Considered>,
    /// The built-in piano stands in because no plugin could be loaded (not chosen): it is not
    /// remembered as the choice.
    pub fallback: bool,
    /// For the report: why it is not the one used last time (empty: nothing to say).
    pub note: String,
}

/// Whether `arg` asks for the built-in piano: its id or "built-in".
pub const fn is_built_in(arg: &str) -> bool {
    arg.eq_ignore_ascii_case(BUILT_IN_PIANO) || arg.eq_ignore_ascii_case("built-in")
}

/// Chooses the instrument:
///
/// - `asked` (the command line, or a click in the window): exactly that one, or the error;
/// - else `remembered` (used last time), if it still loads;
/// - else the first installed instrument; and when there is none or it does not load, the
///   built-in piano as a stand-in (`fallback`), if this build has it, else the error.
///
/// # Errors
///
/// When the instrument asked for cannot be loaded, or nothing can.
pub fn choose<S: Source>(src: &mut S, asked: Option<&str>, remembered: Option<&str>) -> Result<Chosen<S::Instrument>> {
    let chosen = |instrument, considered| Chosen {
        instrument,
        considered,
        fallback: false,
        note: String::new(),
    };
    if let Some(a) = asked {
        return load(src, a).map(|(i, c)| chosen(i, c));
    }
    let mut note = String::new();
    if let Some(last) = remembered {
        match load(src, last) {
            Ok((i, mut considered)) => {
                for c in &mut considered {
                    c.why = match c.verdict {
                        Verdict::Used => "the instrument used last time".into(),
                        _ => "not the one used last time".into(),
                    };
                }
                return Ok(chosen(i, considered));
            }
            Err(e) => {
                log::warn!("the instrument used last time: {e:#}");
                note = "the one used last time failed to load".into();
            }
        }
    }
    match src.plugin(None) {
        Ok((i, mut considered)) => {
            if src.has_piano() {
                considered.push(piano_entry(
                    Verdict::NotUsed,
                    "plays when no plugin can be loaded".into(),
                ));
            }
            Ok(Chosen {
                note,
                ..chosen(i, considered)
            })
        }
        Err(e) => {
            let Ok(piano) = src.piano() else {
                return Err(e);
            };
            log::info!("no plugin: {e:#}");
            let why = format!("no VST3 instrument could be loaded: {e:#}");
            Ok(Chosen {
                instrument: piano,
                considered: vec![piano_entry(Verdict::Used, why)],
                fallback: true,
                note,
            })
        }
    }
}

/// The instrument `arg` names: the built-in piano or a plugin.
fn load<S: Source>(src: &mut S, arg: &str) -> Result<(S::Instrument, Vec<Considered>)> {
    if is_built_in(arg) {
        let piano = src.piano()?;
        return Ok((piano, vec![piano_entry(Verdict::Used, "asked for".into())]));
    }
    src.plugin(Some(arg))
}

fn piano_entry(verdict: Verdict, why: String) -> Considered {
    Considered {
        name: crate::piano::NAME.to_owned(),
        verdict,
        why,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installed plugins by name, each loading or not; the built-in piano or not.
    struct Fake {
        plugins: Vec<(&'static str, bool)>,
        piano: bool,
    }

    fn fake(plugins: &[(&'static str, bool)], piano: bool) -> Fake {
        Fake {
            plugins: plugins.to_vec(),
            piano,
        }
    }

    impl Source for Fake {
        type Instrument = String;

        fn plugin(&mut self, arg: Option<&str>) -> Result<(String, Vec<Considered>)> {
            let Some(&(name, loads)) =
                arg.map_or_else(|| self.plugins.first(), |a| self.plugins.iter().find(|p| p.0 == a))
            else {
                bail!("no such plugin");
            };
            if !loads {
                bail!("{name} failed to load");
            }
            let considered = vec![Considered {
                name: name.into(),
                verdict: Verdict::Used,
                why: "picked".into(),
            }];
            Ok((name.into(), considered))
        }

        fn piano(&mut self) -> Result<String> {
            if self.piano {
                Ok("piano".into())
            } else {
                bail!("no piano")
            }
        }

        fn has_piano(&self) -> bool {
            self.piano
        }
    }

    /// (instrument, stand-in?) or the error.
    fn pick(src: &mut Fake, asked: Option<&str>, remembered: Option<&str>) -> Result<(String, bool), String> {
        choose(src, asked, remembered)
            .map(|c| (c.instrument, c.fallback))
            .map_err(|e| e.to_string())
    }

    #[expect(clippy::unnecessary_wraps, reason = "compared with what pick returns")]
    fn ok(name: &str, fallback: bool) -> Result<(String, bool), String> {
        Ok((name.to_owned(), fallback))
    }

    #[test]
    fn what_is_asked_for_is_used_or_is_an_error() {
        let mut src = fake(&[("Surge", true), ("Broken", false)], true);
        assert_eq!(pick(&mut src, Some("Surge"), Some("Dexed")), ok("Surge", false));
        // No stand-in for an explicit choice: the error says what went wrong.
        assert!(pick(&mut src, Some("Broken"), None).is_err());
        assert!(pick(&mut src, Some("Missing"), Some("Surge")).is_err());
    }

    #[test]
    fn the_built_in_piano_asked_for_is_a_choice_not_a_stand_in() {
        let mut src = fake(&[("Surge", true)], true);
        assert_eq!(pick(&mut src, Some("built-in"), None), ok("piano", false));
        assert_eq!(pick(&mut src, Some(BUILT_IN_PIANO), None), ok("piano", false));
        assert_eq!(pick(&mut src, None, Some(BUILT_IN_PIANO)), ok("piano", false));
        // A build without it.
        assert!(pick(&mut fake(&[("Surge", true)], false), Some("built-in"), None).is_err());
    }

    #[test]
    fn the_one_used_last_time_comes_back_if_it_still_loads() {
        let mut src = fake(&[("Surge", true), ("Dexed", true)], true);
        let c = choose(&mut src, None, Some("Dexed")).unwrap();
        assert_eq!((c.instrument.as_str(), c.fallback), ("Dexed", false));
        assert_eq!(c.note, "");
        assert!(c.considered[0].why.contains("last time"));
        // Gone or broken: the automatic choice, and the report says why.
        let c = choose(&mut src, None, Some("Uninstalled")).unwrap();
        assert_eq!(c.instrument, "Surge");
        assert!(c.note.contains("last time"), "{}", c.note);
    }

    #[test]
    fn without_a_choice_the_first_instrument_plays() {
        let mut src = fake(&[("Surge", true), ("Dexed", true)], true);
        let c = choose(&mut src, None, None).unwrap();
        assert_eq!((c.instrument.as_str(), c.fallback), ("Surge", false));
        // The piano is listed as what would stand in.
        assert!(
            c.considered
                .iter()
                .any(|c| c.name == crate::piano::NAME && c.verdict == Verdict::NotUsed)
        );
    }

    #[test]
    fn the_piano_stands_in_when_no_plugin_loads() {
        assert_eq!(pick(&mut fake(&[], true), None, None), ok("piano", true));
        assert_eq!(
            pick(&mut fake(&[("Broken", false)], true), None, None),
            ok("piano", true)
        );
        assert_eq!(pick(&mut fake(&[], true), None, Some("Gone")), ok("piano", true));
        // Without the piano: the error.
        assert!(pick(&mut fake(&[], false), None, None).is_err());
        assert!(pick(&mut fake(&[("Broken", false)], false), None, None).is_err());
    }
}
