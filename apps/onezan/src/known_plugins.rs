//! What onezan remembers about the installed plugins between starts, in
//! `<data dir>/plugins.json`: what scanning each bundle found (so a start does not load every
//! plugin again), and the plugins that crashed onezan.
//!
//! Plugins run inside onezan's process: one that crashes while it is scanned, loaded or opens
//! its editor takes onezan down, and would again at every start. So before each of these the
//! bundle is written down (`plugins.busy`, see [`guard`]), and the note is removed after. A note
//! still there at the next start names the plugin that crashed. From then on it is not scanned,
//! not chosen automatically and not brought back as the one used last time, until its binary
//! changes (an update) or it is asked for by name or with a click ([`forgive`]).
//!
//! Not covered: a crash while the plugin plays (on the audio thread) or later in its editor. Two
//! onezans scanning at the same moment could take each other's note for a crash.
//!
//! Best effort, like the settings: a file that cannot be read is started afresh, and without a
//! data folder plugins are scanned at every start and nothing is remembered.

use crate::settings::data_dir;
use onezan_vst3_host::PluginInfo;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::{fs, io};

const FILE: &str = "plugins.json";
const BUSY: &str = "plugins.busy";
/// Raised when what a scan records changes: older files are scanned again (crashes are kept).
const VERSION: u32 = 1;

/// What onezan was doing with a plugin when it crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Scanning,
    Loading,
    OpeningEditor,
}

impl Activity {
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Scanning => "while it was scanned",
            Self::Loading => "while it was loading",
            Self::OpeningEditor => "while it opened its editor",
        }
    }
}

/// The plugins found, and the ones skipped because they crashed onezan.
#[derive(Debug, Default, Clone)]
pub struct Scan {
    pub plugins: Vec<PluginInfo>,
    /// Bundles that crashed onezan, and when. One that crashed while loading is in `plugins` too;
    /// one that crashed while it was scanned is only here.
    pub crashed: Vec<(PathBuf, Activity)>,
}

impl Scan {
    /// When `bundle` crashed onezan; `None`: it didn't (or was updated since).
    pub fn crash(&self, bundle: &Path) -> Option<Activity> {
        let key = canonical(bundle);
        self.crashed.iter().find(|(b, _)| *b == key).map(|&(_, a)| a)
    }
}

/// The installed VST3 plugins: from the cache where a bundle is unchanged, else scanned (with a
/// [`guard`]). Bundles that crashed onezan are skipped.
pub fn installed() -> Scan {
    let bundles = onezan_vst3_host::find_bundles(&onezan_vst3_host::default_search_paths());
    let Some(dir) = data_dir() else {
        return Scan {
            plugins: onezan_vst3_host::scan(&bundles),
            crashed: Vec::new(),
        };
    };
    take_note_once(&dir);
    let mut known = Known::open(&dir, &onezan_vst3_host::fingerprint);
    let scan = known.scan(&bundles, &mut |b| {
        onezan_vst3_host::scan_bundle(b).map_err(|e| e.to_string())
    });
    known.save();
    scan
}

/// Tries `bundle` again: it was asked for, so it is no longer skipped.
pub fn forgive(bundle: &Path) {
    if let Some(dir) = data_dir() {
        take_note_once(&dir);
        let mut known = Known::open(&dir, &onezan_vst3_host::fingerprint);
        known.forgive(&canonical(bundle));
        known.save();
    }
}

/// Writes down that onezan is `during` with `bundle`, until the guard is dropped. Hold it over
/// any call into the plugin that runs its code for the first time.
#[must_use = "the note is removed when the guard is dropped"]
pub fn guard(bundle: &Path, during: Activity) -> Guard {
    data_dir().map_or(Guard { path: None }, |dir| {
        take_note_once(&dir);
        guard_in(&dir, bundle, during)
    })
}

/// Removes the note of [`guard`] when dropped.
pub struct Guard {
    path: Option<PathBuf>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            let _ = fs::remove_file(p);
        }
    }
}

fn guard_in(dir: &Path, bundle: &Path, during: Activity) -> Guard {
    let path = dir.join(BUSY);
    let busy = Busy {
        bundle: canonical(bundle),
        during,
    };
    let written = serde_json::to_string(&busy)
        .map_err(io::Error::other)
        .and_then(|text| fs::create_dir_all(dir).and_then(|()| fs::write(&path, text)));
    match written {
        Ok(()) => Guard { path: Some(path) },
        Err(e) => {
            log::warn!("cannot write {}: {e}", path.display());
            Guard { path: None }
        }
    }
}

/// Once per run, before onezan scans or loads anything: a note left behind is a crash.
fn take_note_once(dir: &Path) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let mut known = Known::open(dir, &onezan_vst3_host::fingerprint);
        known.take_note();
        known.save();
    });
}

fn canonical(bundle: &Path) -> PathBuf {
    bundle.canonicalize().unwrap_or_else(|_| bundle.to_path_buf())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Crash {
    /// The bundle's [`onezan_vst3_host::fingerprint`] then: a new one means it was updated.
    fingerprint: Option<String>,
    during: Activity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    fingerprint: String,
    plugins: Vec<Found>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Found {
    name: String,
    vendor: String,
    class_id: String,
    sub_categories: String,
}

impl Found {
    fn info(&self, bundle: &Path) -> PluginInfo {
        PluginInfo {
            bundle: bundle.to_path_buf(),
            name: self.name.clone(),
            vendor: self.vendor.clone(),
            class_id: self.class_id.clone(),
            sub_categories: self.sub_categories.clone(),
        }
    }
}

impl From<&PluginInfo> for Found {
    fn from(p: &PluginInfo) -> Self {
        Self {
            name: p.name.clone(),
            vendor: p.vendor.clone(),
            class_id: p.class_id.clone(),
            sub_categories: p.sub_categories.clone(),
        }
    }
}

/// `plugins.json`. Bundles by canonical path.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    version: u32,
    bundles: BTreeMap<PathBuf, Entry>,
    crashed: BTreeMap<PathBuf, Crash>,
}

/// `plugins.busy`: what onezan is doing, while it does it.
#[derive(Serialize, Deserialize)]
struct Busy {
    bundle: PathBuf,
    during: Activity,
}

type Fingerprint<'a> = dyn Fn(&Path) -> Option<String> + 'a;

/// The file, read, with the changes to write back.
struct Known<'a> {
    dir: &'a Path,
    file: File,
    changed: bool,
    fingerprint: &'a Fingerprint<'a>,
}

impl<'a> Known<'a> {
    fn open(dir: &'a Path, fingerprint: &'a Fingerprint<'a>) -> Self {
        let path = dir.join(FILE);
        let mut file = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("cannot read {}: {e}; the plugins are scanned again", path.display());
                File::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => File::default(),
            Err(e) => {
                log::warn!("cannot read {}: {e}", path.display());
                File::default()
            }
        };
        let changed = file.version != VERSION;
        if changed {
            file.bundles.clear();
            file.version = VERSION;
        }
        Self {
            dir,
            file,
            changed,
            fingerprint,
        }
    }

    /// Takes in the note of a [`guard`] that was never dropped: that plugin crashed onezan.
    fn take_note(&mut self) {
        let path = self.dir.join(BUSY);
        let Ok(text) = fs::read_to_string(&path) else { return };
        let _ = fs::remove_file(&path);
        let Ok(busy) = serde_json::from_str::<Busy>(&text) else {
            log::warn!("{} is damaged; ignored", path.display());
            return;
        };
        log::warn!(
            "{} crashed onezan last time, {}: skipped until it is asked for",
            busy.bundle.display(),
            busy.during.describe()
        );
        let crash = Crash {
            fingerprint: (self.fingerprint)(&busy.bundle),
            during: busy.during,
        };
        self.file.crashed.insert(busy.bundle, crash);
        self.changed = true;
    }

    /// When `key` crashed onezan, unless it was updated since (then it is tried again).
    fn crash(&mut self, key: &Path) -> Option<Activity> {
        let c = self.file.crashed.get(key)?;
        if c.fingerprint == (self.fingerprint)(key) {
            return Some(c.during);
        }
        log::info!("{} was updated since it crashed onezan: trying it again", key.display());
        self.file.crashed.remove(key);
        self.changed = true;
        None
    }

    fn forgive(&mut self, key: &Path) {
        if self.file.crashed.remove(key).is_some() {
            self.changed = true;
        }
    }

    /// The plugins in `bundles`: unchanged bundles from the file, the others through `load`
    /// (under a guard). Bundles that are gone are dropped from the file.
    fn scan(&mut self, bundles: &[PathBuf], load: &mut dyn FnMut(&Path) -> Result<Vec<PluginInfo>, String>) -> Scan {
        let mut keys: Vec<PathBuf> = Vec::new();
        for k in bundles.iter().map(|b| canonical(b)) {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let mut out = Scan::default();
        for key in &keys {
            if let Some(during) = self.crash(key) {
                out.crashed.push((key.clone(), during));
                if during == Activity::Scanning {
                    continue;
                }
            }
            let fingerprint = (self.fingerprint)(key);
            if let Some(e) = self.file.bundles.get(key)
                && fingerprint.as_ref() == Some(&e.fingerprint)
            {
                out.plugins.extend(e.plugins.iter().map(|c| c.info(key)));
                continue;
            }
            let found = {
                let _busy = guard_in(self.dir, key, Activity::Scanning);
                load(key)
            };
            match found {
                Ok(plugins) => {
                    if let Some(fingerprint) = fingerprint {
                        let entry = Entry {
                            fingerprint,
                            plugins: plugins.iter().map(Found::from).collect(),
                        };
                        self.file.bundles.insert(key.clone(), entry);
                        self.changed = true;
                    }
                    out.plugins.extend(plugins.into_iter().map(|p| PluginInfo {
                        bundle: key.clone(),
                        ..p
                    }));
                }
                // Not remembered: it may load once a missing library is installed.
                Err(e) => {
                    log::warn!("{e}");
                    if self.file.bundles.remove(key).is_some() {
                        self.changed = true;
                    }
                }
            }
        }
        let before = (self.file.bundles.len(), self.file.crashed.len());
        self.file.bundles.retain(|b, _| keys.contains(b));
        self.file
            .crashed
            .retain(|b, _| keys.contains(b) || Path::new(b).exists());
        if before != (self.file.bundles.len(), self.file.crashed.len()) {
            self.changed = true;
        }
        out
    }

    fn save(&self) {
        if !self.changed {
            return;
        }
        let path = self.dir.join(FILE);
        let tmp = path.with_extension("json.tmp");
        let written = serde_json::to_string_pretty(&self.file)
            .map_err(io::Error::other)
            .and_then(|text| fs::create_dir_all(self.dir).and_then(|()| fs::write(&tmp, text)))
            .and_then(|()| fs::rename(&tmp, &path));
        if let Err(e) = written {
            log::warn!("cannot write {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

    use super::*;
    use std::cell::RefCell;

    /// A fresh folder for one test.
    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("onezan-known-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Bundles that don't exist on disk (so their paths stay as given), with a version each: the
    /// fingerprint.
    struct Fake {
        versions: RefCell<BTreeMap<PathBuf, String>>,
        loads: RefCell<Vec<PathBuf>>,
    }

    impl Fake {
        fn new(bundles: &[&str]) -> Self {
            Self {
                versions: RefCell::new(bundles.iter().map(|b| (PathBuf::from(b), "1".to_owned())).collect()),
                loads: RefCell::default(),
            }
        }

        fn bundles(&self) -> Vec<PathBuf> {
            self.versions.borrow().keys().cloned().collect()
        }

        fn update(&self, bundle: &str) {
            self.versions.borrow_mut().insert(bundle.into(), "2".into());
        }

        fn fingerprint(&self, b: &Path) -> Option<String> {
            self.versions.borrow().get(b).cloned()
        }

        /// One instrument per bundle, named after it; "broken" ones fail to load.
        fn load(&self, b: &Path) -> Result<Vec<PluginInfo>, String> {
            self.loads.borrow_mut().push(b.to_path_buf());
            let name = b.file_stem().unwrap().to_string_lossy().into_owned();
            if name.contains("broken") {
                return Err(format!("{name} does not load"));
            }
            Ok(vec![PluginInfo {
                bundle: b.to_path_buf(),
                name: name.clone(),
                vendor: "Fake".into(),
                class_id: name.to_uppercase(),
                sub_categories: "Instrument".into(),
            }])
        }

        /// A start of onezan: takes in a note left behind, then scans.
        fn start(&self, dir: &Path) -> Scan {
            let fingerprint = |b: &Path| self.fingerprint(b);
            let mut known = Known::open(dir, &fingerprint);
            known.take_note();
            let scan = known.scan(&self.bundles(), &mut |b| self.load(b));
            known.save();
            scan
        }

        fn loads(&self) -> usize {
            self.loads.borrow().len()
        }
    }

    fn names(scan: &Scan) -> Vec<&str> {
        scan.plugins.iter().map(|p| p.name.as_str()).collect()
    }

    /// What a crash leaves behind: the note of a guard that was never dropped.
    fn crash_while(dir: &Path, bundle: &str, during: Activity) {
        std::mem::forget(guard_in(dir, Path::new(bundle), during));
    }

    #[test]
    fn an_unchanged_bundle_is_not_loaded_again() {
        let d = dir("cache");
        let fake = Fake::new(&["/p/a.vst3", "/p/b.vst3"]);
        assert_eq!(names(&fake.start(&d)), ["a", "b"]);
        assert_eq!(fake.loads(), 2);
        assert_eq!(names(&fake.start(&d)), ["a", "b"]);
        assert_eq!(fake.loads(), 2, "the second start comes from the file");
        // An update is scanned again, the other bundle still comes from the file.
        fake.update("/p/b.vst3");
        assert_eq!(names(&fake.start(&d)), ["a", "b"]);
        assert_eq!(fake.loads(), 3);
    }

    #[test]
    fn a_bundle_that_fails_to_load_is_tried_at_each_start() {
        let d = dir("broken");
        let fake = Fake::new(&["/p/a.vst3", "/p/broken.vst3"]);
        assert_eq!(names(&fake.start(&d)), ["a"]);
        assert_eq!(names(&fake.start(&d)), ["a"]);
        assert_eq!(fake.loads(), 3, "a once, broken twice");
    }

    #[test]
    fn while_a_bundle_is_scanned_the_note_names_it() {
        let d = dir("note");
        let fake = Fake::new(&["/p/a.vst3"]);
        let fingerprint = |b: &Path| fake.fingerprint(b);
        let mut known = Known::open(&d, &fingerprint);
        let mut seen = None;
        known.scan(&fake.bundles(), &mut |b| {
            seen = fs::read_to_string(d.join(BUSY)).ok();
            fake.load(b)
        });
        let seen = seen.expect("a note while scanning");
        assert!(seen.contains("a.vst3") && seen.contains("scanning"), "{seen}");
        assert!(!d.join(BUSY).exists(), "removed after");
    }

    #[test]
    fn a_bundle_that_crashed_the_scan_is_skipped_until_it_is_updated() {
        let d = dir("crash-scan");
        let fake = Fake::new(&["/p/a.vst3", "/p/b.vst3"]);
        crash_while(&d, "/p/b.vst3", Activity::Scanning);
        let scan = fake.start(&d);
        assert_eq!(names(&scan), ["a"]);
        assert_eq!(scan.crash(Path::new("/p/b.vst3")), Some(Activity::Scanning));
        assert_eq!(fake.loads(), 1, "b is not loaded");
        assert!(!d.join(BUSY).exists());
        // Still skipped at the next start; tried again once updated.
        assert_eq!(names(&fake.start(&d)), ["a"]);
        fake.update("/p/b.vst3");
        let scan = fake.start(&d);
        assert_eq!(names(&scan), ["a", "b"]);
        assert_eq!(scan.crash(Path::new("/p/b.vst3")), None);
    }

    #[test]
    fn a_plugin_that_crashed_while_loading_is_listed_as_such_until_forgiven() {
        let d = dir("crash-load");
        let fake = Fake::new(&["/p/a.vst3", "/p/b.vst3"]);
        fake.start(&d);
        crash_while(&d, "/p/b.vst3", Activity::Loading);
        let scan = fake.start(&d);
        assert_eq!(names(&scan), ["a", "b"], "still listed, to be asked for");
        assert_eq!(scan.crash(Path::new("/p/b.vst3")), Some(Activity::Loading));
        let fingerprint = |b: &Path| fake.fingerprint(b);
        let mut known = Known::open(&d, &fingerprint);
        known.forgive(Path::new("/p/b.vst3"));
        known.save();
        assert_eq!(fake.start(&d).crash(Path::new("/p/b.vst3")), None);
    }

    #[test]
    fn a_damaged_file_means_scanning_again() {
        let d = dir("damaged");
        fs::write(d.join(FILE), "{ not json").unwrap();
        fs::write(d.join(BUSY), "nor this").unwrap();
        let fake = Fake::new(&["/p/a.vst3"]);
        let scan = fake.start(&d);
        assert_eq!(names(&scan), ["a"]);
        assert_eq!(scan.crashed.len(), 0, "{:?}", scan.crashed);
        assert_eq!(names(&fake.start(&d)), ["a"]);
        assert_eq!(fake.loads(), 1, "the file was written afresh");
    }

    #[test]
    fn an_older_file_is_scanned_again_but_keeps_its_crashes() {
        let d = dir("version");
        let fake = Fake::new(&["/p/a.vst3", "/p/b.vst3"]);
        crash_while(&d, "/p/b.vst3", Activity::Scanning);
        fake.start(&d);
        let text = fs::read_to_string(d.join(FILE)).unwrap();
        fs::write(d.join(FILE), text.replace("\"version\": 1", "\"version\": 0")).unwrap();
        let scan = fake.start(&d);
        assert_eq!(fake.loads(), 2, "a again");
        assert_eq!(scan.crash(Path::new("/p/b.vst3")), Some(Activity::Scanning));
    }
}
