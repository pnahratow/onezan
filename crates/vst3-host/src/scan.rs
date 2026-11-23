//! Finding installed VST3 plugins.

use crate::module::Module;
use std::path::{Path, PathBuf};

/// One plugin class found in a bundle.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub bundle: PathBuf,
    pub name: String,
    pub vendor: String,
    /// The class id, in upper-case hex.
    pub class_id: String,
    /// VST3 sub-categories, e.g. `Instrument|Synth`.
    pub sub_categories: String,
}

impl PluginInfo {
    #[must_use]
    pub fn is_instrument(&self) -> bool {
        self.sub_categories.split('|').any(|c| c == "Instrument")
    }
}

/// The standard VST3 folders for this platform, plus `$VST3_PATH` (path-list separated).
#[must_use]
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("VST3_PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    #[cfg(windows)]
    {
        for var in ["CommonProgramFiles", "CommonProgramFiles(x86)"] {
            if let Some(p) = std::env::var_os(var) {
                out.push(PathBuf::from(p).join("VST3"));
            }
        }
        if let Some(p) = std::env::var_os("LOCALAPPDATA") {
            out.push(PathBuf::from(p).join("Programs").join("Common").join("VST3"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(".vst3"));
        }
        out.push("/usr/lib/vst3".into());
        out.push("/usr/local/lib/vst3".into());
    }
    out
}

/// Every `.vst3` bundle under `dirs` (vendor subfolders included).
#[must_use]
pub fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    const MAX_DEPTH: u32 = 4;
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vst3")) {
                out.push(p);
            } else if p.is_dir() && depth < MAX_DEPTH {
                walk(&p, depth.saturating_add(1), out);
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, 0, &mut out);
    }
    out.dedup();
    out
}

/// Loads every bundle and lists its audio processor classes. Bundles that fail to load are
/// logged and skipped. Loading runs plugin code in this process.
#[must_use]
pub fn scan(bundles: &[PathBuf]) -> Vec<PluginInfo> {
    let mut out = Vec::new();
    for b in bundles {
        match Module::load(b) {
            Ok(m) => {
                for c in m.classes() {
                    if c.category != "Audio Module Class" {
                        continue;
                    }
                    out.push(PluginInfo {
                        bundle: b.clone(),
                        name: c.name,
                        vendor: c.vendor,
                        class_id: crate::util::class_id_string(&c.cid),
                        sub_categories: c.sub_categories,
                    });
                }
            }
            Err(e) => log::warn!("{e}"),
        }
    }
    out
}
