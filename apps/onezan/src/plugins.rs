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

/// A loaded instrument, with the verdict on each one considered.
pub struct Loaded {
    pub instrument: Instrument,
    pub considered: Vec<Considered>,
    /// The built-in piano stands in because no plugin could be loaded (not chosen): it is not
    /// remembered as the choice.
    pub fallback: bool,
}

/// Whether `arg` asks for the built-in piano: its id or "built-in".
pub const fn is_built_in(arg: &str) -> bool {
    arg.eq_ignore_ascii_case(BUILT_IN_PIANO) || arg.eq_ignore_ascii_case("built-in")
}

/// Loads the instrument `arg` names: the built-in piano, or a plugin (a path, a class id or part
/// of a name). Without `arg`: the first installed instrument, and when there is none or it does
/// not load, the built-in piano.
pub fn load(arg: Option<&str>) -> Result<Loaded> {
    let piano_entry = |verdict, why: String| Considered {
        name: crate::piano::NAME.to_owned(),
        verdict,
        why,
    };
    if arg.is_some_and(is_built_in) {
        return Ok(Loaded {
            instrument: Instrument::Piano(crate::piano::load()?),
            considered: vec![piano_entry(Verdict::Used, "asked for".into())],
            fallback: false,
        });
    }
    let plugin = resolve(arg).and_then(|(bundle, class, considered)| {
        let p = Plugin::load(&bundle, class.as_deref()).with_context(|| format!("loading {}", bundle.display()))?;
        Ok((p, considered))
    });
    match plugin {
        Ok((p, mut considered)) => {
            if crate::piano::available() {
                considered.push(piano_entry(
                    Verdict::NotUsed,
                    "plays when no plugin can be loaded".into(),
                ));
            }
            Ok(Loaded {
                instrument: Instrument::Plugin(p),
                considered,
                fallback: false,
            })
        }
        Err(e) if arg.is_none() => {
            let Ok(piano) = crate::piano::load() else {
                return Err(e);
            };
            log::info!("no plugin: {e:#}");
            let considered = vec![piano_entry(
                Verdict::Used,
                format!("no VST3 instrument could be loaded: {e:#}"),
            )];
            Ok(Loaded {
                instrument: Instrument::Piano(piano),
                considered,
                fallback: true,
            })
        }
        Err(e) => Err(e),
    }
}
