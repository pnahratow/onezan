//! Which instrument to load: the installed plugins, the built-in piano, and the one an argument
//! (or nothing) picks, with the verdict on each for the startup report.

use crate::known_plugins::{Activity, Scan};
use anyhow::{Context, Result, bail};
use onezan_engine::{BUILT_IN_PIANO, Considered, Instrument, Verdict};
use onezan_vst3_host::{Plugin, PluginInfo};
use std::path::{Path, PathBuf};

/// The installed VST3 plugins, and the ones skipped because they crashed onezan.
pub fn installed() -> Scan {
    crate::known_plugins::installed()
}

/// Resolves the plugin argument to (bundle, class id), with the verdict on each instrument.
pub fn resolve(scan: &Scan, arg: Option<&str>) -> Result<(PathBuf, Option<String>, Vec<Considered>)> {
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
    let plugins = &scan.plugins;
    let (pick, considered) = pick(scan, arg);
    match pick.and_then(|i| plugins.get(i)) {
        Some(p) => Ok((p.bundle.clone(), Some(p.class_id.clone()), considered)),
        None if arg.is_none() && plugins.iter().any(PluginInfo::is_instrument) => {
            bail!("every installed VST3 instrument crashed onezan before, so none is loaded by itself")
        }
        None if arg.is_none() => bail!("no VST3 instrument installed (try `onezan list --all`)"),
        None => bail!(
            "no installed plugin matches {:?} (see `onezan list`)",
            arg.unwrap_or_default()
        ),
    }
}

/// The plugin `arg` names (a class id or part of a name), or without one the first installed
/// instrument that has not crashed onezan; with the verdict on each instrument.
fn pick(scan: &Scan, arg: Option<&str>) -> (Option<usize>, Vec<Considered>) {
    let plugins = &scan.plugins;
    let crashed = |p: &PluginInfo| scan.crash(&p.bundle);
    let pick = arg.map_or_else(
        || plugins.iter().position(|p| p.is_instrument() && crashed(p).is_none()),
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
    let mut considered: Vec<Considered> = plugins
        .iter()
        .enumerate()
        .filter(|&(i, p)| p.is_instrument() || Some(i) == pick)
        .map(|(i, p)| {
            let (verdict, why) = match (Some(i) == pick, arg, crashed(p)) {
                (true, Some(a), _) => (Verdict::Used, format!("matches \"{a}\"")),
                (true, None, _) => (Verdict::Used, "the first installed instrument".to_string()),
                (false, None, Some(during)) => (Verdict::NotUsed, skipped(during)),
                (false, Some(a), _) => (Verdict::NotUsed, format!("does not match \"{a}\"")),
                (false, None, None) => (Verdict::NotUsed, "another instrument was picked first".to_string()),
            };
            Considered {
                name: format!("{} ({})", p.name, p.vendor),
                verdict,
                why,
            }
        })
        .collect();
    // Bundles that crashed onezan while they were scanned: no names known, only the file.
    considered.extend(
        scan.crashed
            .iter()
            .filter(|(_, during)| *during == Activity::Scanning)
            .map(|(bundle, during)| Considered {
                name: bundle
                    .file_name()
                    .map_or_else(|| bundle.display().to_string(), |n| n.to_string_lossy().into_owned()),
                verdict: Verdict::NotUsed,
                why: skipped(*during),
            }),
    );
    (pick, considered)
}

fn skipped(during: Activity) -> String {
    format!(
        "crashed onezan last time, {}; skipped until you try it again under Instrument",
        during.describe()
    )
}

/// Where instruments come from, for [`choose`]. [`Installed`] loads the real ones; the tests
/// use a fake, so the rules of choosing are checked without plugins.
pub trait Source {
    type Instrument;
    /// The plugin `arg` names (a path, a class id or part of a name); without one, the first
    /// installed instrument. With the verdict on each installed one.
    fn plugin(&mut self, arg: Option<&str>) -> Result<(Self::Instrument, Vec<Considered>)>;
    /// Why the plugin `arg` names is skipped unless asked for (it crashed onezan); `None`: it
    /// isn't.
    fn crashed(&mut self, arg: &str) -> Option<String>;
    /// The built-in piano; an error when this build has none.
    fn piano(&mut self) -> Result<Self::Instrument>;
    /// Whether this build has the built-in piano (without loading it).
    fn has_piano(&self) -> bool;
}

/// The installed plugins and this build's built-in piano.
#[derive(Default)]
pub struct Installed {
    /// Scanned once, when first needed.
    scan: Option<Scan>,
}

impl Installed {
    fn scan(&mut self) -> &Scan {
        self.scan.get_or_insert_with(installed)
    }
}

impl Source for Installed {
    type Instrument = Instrument;

    fn plugin(&mut self, arg: Option<&str>) -> Result<(Instrument, Vec<Considered>)> {
        let (bundle, class, considered) = resolve(self.scan(), arg)?;
        if arg.is_some() {
            // Asked for: tried again even if it crashed onezan before.
            crate::known_plugins::forgive(&bundle);
        }
        let loaded = {
            let _busy = crate::known_plugins::guard(&bundle, Activity::Loading);
            Plugin::load(&bundle, class.as_deref())
        };
        let p = loaded.with_context(|| format!("loading {}", bundle.display()))?;
        Ok((Instrument::Plugin(p), considered))
    }

    fn crashed(&mut self, arg: &str) -> Option<String> {
        if is_built_in(arg) {
            return None;
        }
        let scan = self.scan();
        let (i, _) = pick(scan, Some(arg));
        let p = scan.plugins.get(i?)?;
        scan.crash(&p.bundle)
            .map(|during| format!("crashed onezan {}", during.describe()))
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
/// - else `remembered` (used last time), if it still loads and did not crash onezan;
/// - else the first installed instrument that did not crash onezan; and when there is none or it does not load, the
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
    if let Some(last) = remembered
        && let Some(why) = src.crashed(last)
    {
        log::warn!("the instrument used last time {why}: skipped");
        note = format!("the one used last time {why}; skipped until you try it again under Instrument");
    } else if let Some(last) = remembered {
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
        /// The plugins that crashed onezan.
        crashed: Vec<&'static str>,
    }

    fn fake(plugins: &[(&'static str, bool)], piano: bool) -> Fake {
        Fake {
            plugins: plugins.to_vec(),
            piano,
            crashed: Vec::new(),
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

        fn crashed(&mut self, arg: &str) -> Option<String> {
            self.crashed.contains(&arg).then(|| "crashed onezan".to_owned())
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

    #[test]
    fn the_one_used_last_time_is_skipped_if_it_crashed_onezan() {
        let mut src = fake(&[("Surge", true), ("Dexed", true)], true);
        src.crashed = vec!["Dexed"];
        let c = choose(&mut src, None, Some("Dexed")).unwrap();
        assert_eq!(c.instrument, "Surge");
        assert!(c.note.contains("crashed onezan"), "{}", c.note);
        // Asked for, it is tried again.
        assert_eq!(pick(&mut src, Some("Dexed"), None), ok("Dexed", false));
    }

    fn info(name: &str, bundle: &str, sub_categories: &str) -> PluginInfo {
        PluginInfo {
            bundle: bundle.into(),
            name: name.into(),
            vendor: "Fake".into(),
            class_id: name.to_uppercase(),
            sub_categories: sub_categories.into(),
        }
    }

    #[test]
    fn the_automatic_pick_passes_over_plugins_that_crashed_onezan() {
        let scan = Scan {
            plugins: vec![
                info("Reverb", "/p/reverb.vst3", "Fx"),
                info("Dexed", "/p/dexed.vst3", "Instrument|Synth"),
                info("Surge", "/p/surge.vst3", "Instrument|Synth"),
            ],
            crashed: vec![
                ("/p/dexed.vst3".into(), Activity::Loading),
                ("/p/odin.vst3".into(), Activity::Scanning),
            ],
        };
        let (i, considered) = super::pick(&scan, None);
        assert_eq!(i, Some(2), "Surge");
        let why = |name: &str| {
            considered
                .iter()
                .find(|c| c.name.starts_with(name))
                .map(|c| (c.verdict, c.why.as_str()))
        };
        assert!(why("Dexed").is_some_and(|(v, w)| v == Verdict::NotUsed && w.contains("while it was loading")));
        assert!(why("odin.vst3").is_some_and(|(v, w)| v == Verdict::NotUsed && w.contains("while it was scanned")));
        assert_eq!(why("Surge").map(|(v, _)| v), Some(Verdict::Used));
        // By name, a crashed one is still found (to be tried again).
        assert_eq!(super::pick(&scan, Some("dexed")).0, Some(1));
        // Only crashed instruments: no automatic pick, and the error says why.
        let only = Scan {
            plugins: vec![info("Dexed", "/p/dexed.vst3", "Instrument")],
            crashed: vec![("/p/dexed.vst3".into(), Activity::OpeningEditor)],
        };
        let e = resolve(&only, None).unwrap_err().to_string();
        assert!(e.contains("crashed"), "{e}");
    }
}
