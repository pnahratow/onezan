//! Which plugin to load: the installed ones, and the one an argument (or nothing) picks, with the
//! verdict on each for the startup report.

use anyhow::{Context, Result, bail};
use onezan_engine::{Considered, Verdict};
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

pub fn load(arg: Option<&str>) -> Result<(Plugin, Vec<Considered>)> {
    let (bundle, class, considered) = resolve(arg)?;
    let plugin = Plugin::load(&bundle, class.as_deref()).with_context(|| format!("loading {}", bundle.display()))?;
    Ok((plugin, considered))
}
