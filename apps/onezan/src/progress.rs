//! Practice history: every note played, with the scale selected at the time, in a SQLite file
//! (`<data dir>/progress.sqlite`). Practice time and the rest are worked out from those notes
//! when they are needed, so a better calculation later also applies to the past.
//!
//! Notes are written by a thread of their own, in batches every couple of seconds and whenever
//! the scale changes, so a crash or power cut loses at most those seconds. Best effort: when the
//! file cannot be opened the app runs without recording, and a damaged file is moved aside
//! (`progress.sqlite.broken`) and a new one started.

use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A gap between two notes longer than this is a pause: it does not count as practice.
pub const PAUSE: Duration = Duration::from_secs(15);

/// How often recorded notes are written to the file.
const FLUSH: Duration = Duration::from_secs(2);

/// The history file in the usual place.
pub fn default_path() -> Option<PathBuf> {
    crate::settings::data_dir().map(|d| d.join("progress.sqlite"))
}

enum Msg {
    Note {
        t_ms: i64,
        key: u8,
        velocity: u8,
    },
    Scale(Option<String>),
    Run(i64, RunRecord),
    /// Write what is left and end.
    Stop,
}

/// Records notes into the history file. Stops (writing what is left) when dropped.
pub struct Recorder {
    tx: Option<Sender<Msg>>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Opens (or creates) the history at `path`. `None`, logged, when that is not possible.
    pub fn start(path: &Path) -> Option<Self> {
        let conn = open(path)
            .map_err(|e| log::warn!("practice history off: cannot open {}: {e}", path.display()))
            .ok()?;
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("onezan-progress".into())
            .spawn(move || write_loop(&conn, &rx))
            .map_err(|e| log::warn!("practice history off: {e}"))
            .ok()?;
        Some(Self {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    /// A function that records one note (key, velocity; 0 for a release), for any thread.
    pub fn note_sink(&self) -> impl Fn(u8, u8) + Send + Sync + 'static {
        let tx = std::sync::Mutex::new(self.tx.clone());
        move |key, velocity| {
            let msg = Msg::Note {
                t_ms: now_ms(),
                key,
                velocity,
            };
            if let Some(tx) = tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
                let _ = tx.send(msg);
            }
        }
    }

    /// Records a scale exercise played through with the metronome.
    pub fn record_run(&self, run: RunRecord) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Run(now_ms(), run));
        }
    }

    /// The scale selected from now on (its name), or none.
    pub fn set_scale(&self, scale: Option<String>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Scale(scale));
        }
    }
}

impl Drop for Recorder {
    /// Writes what is left and waits for it: nothing played before quitting is lost. Notes sent
    /// to a sink after this are dropped.
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Msg::Stop);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// Opens the file and makes sure its table is there. A file that is not a usable database is
/// moved aside and a new one created.
fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match open_checked(path) {
        Ok(c) => Ok(c),
        Err(e) if path.exists() => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(".broken");
            log::warn!(
                "practice history {} unusable ({e}); moved aside, starting a new one",
                path.display()
            );
            let _ = std::fs::rename(path, &aside);
            open_checked(path)
        }
        Err(e) => Err(e),
    }
}

fn open_checked(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL: the window reads while the recorder writes, without either waiting.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
            t_ms INTEGER NOT NULL,      -- when, in ms since 1970 (UTC)
            key INTEGER NOT NULL,       -- MIDI note number
            velocity INTEGER NOT NULL,  -- 1 to 127; 0 for a release
            scale TEXT                  -- the scale selected then, e.g. 'D major'; NULL: none
        );
        CREATE INDEX IF NOT EXISTS notes_by_scale ON notes (scale, t_ms);
        CREATE TABLE IF NOT EXISTS runs (
            t_ms INTEGER NOT NULL,      -- when it ended, in ms since 1970 (UTC)
            scale TEXT NOT NULL,        -- e.g. 'D major'
            exercise TEXT NOT NULL,     -- e.g. 'hands together/2' (hands/octaves)
            bpm INTEGER NOT NULL,
            played INTEGER NOT NULL,    -- notes on time
            notes INTEGER NOT NULL,
            spread_ms REAL,             -- the timing's standard deviation
            loudness REAL,              -- the velocity's standard deviation
            clean INTEGER NOT NULL      -- 1: every note on time and the timing even
        );
        CREATE INDEX IF NOT EXISTS runs_by_exercise ON runs (exercise, scale);
        PRAGMA user_version = 2;",
    )?;
    Ok(conn)
}

fn write_loop(conn: &Connection, rx: &mpsc::Receiver<Msg>) {
    let mut scale: Option<String> = None;
    let mut batch: Vec<(i64, u8, u8, Option<String>)> = Vec::new();
    loop {
        let (flush, done) = match rx.recv_timeout(FLUSH) {
            Ok(Msg::Note { t_ms, key, velocity }) => {
                batch.push((t_ms, key, velocity, scale.clone()));
                (batch.len() >= 1000, false)
            }
            Ok(Msg::Scale(s)) => {
                scale = s;
                (true, false)
            }
            Ok(Msg::Run(t_ms, run)) => {
                if let Err(e) = write_run(conn, t_ms, &run) {
                    log::warn!("practice history: cannot write a run: {e}");
                }
                (false, false)
            }
            Err(RecvTimeoutError::Timeout) => (true, false),
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => (true, true),
        };
        if flush && !batch.is_empty() {
            if let Err(e) = write(conn, &batch) {
                log::warn!("practice history: cannot write {} notes: {e}", batch.len());
            }
            batch.clear();
        }
        if done {
            return;
        }
    }
}

fn write(conn: &Connection, batch: &[(i64, u8, u8, Option<String>)]) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached("INSERT INTO notes (t_ms, key, velocity, scale) VALUES (?1, ?2, ?3, ?4)")?;
        for (t, key, velocity, scale) in batch {
            stmt.execute(params![t, key, velocity, scale])?;
        }
    }
    tx.commit()
}

/// A scale exercise played through along with the metronome, and how it went.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRecord {
    pub scale: String,
    /// The exercise's id: `hands/octaves`, e.g. "contrary motion/2".
    pub exercise: String,
    pub bpm: u32,
    pub played: u32,
    pub notes: u32,
    pub spread_ms: Option<f64>,
    pub loudness: Option<f64>,
    pub clean: bool,
}

fn write_run(conn: &Connection, t_ms: i64, r: &RunRecord) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO runs (t_ms, scale, exercise, bpm, played, notes, spread_ms, loudness, clean)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            t_ms,
            r.scale,
            r.exercise,
            r.bpm,
            r.played,
            r.notes,
            r.spread_ms,
            r.loudness,
            r.clean
        ],
    )?;
    Ok(())
}

/// The fastest tempo each scale was played clean at in `exercise` (its id), by scale name.
pub fn clean_tempos(path: &Path, exercise: &str) -> HashMap<String, u32> {
    let read = || -> rusqlite::Result<HashMap<String, u32>> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        // A history from before runs were kept has no such table.
        let has_runs: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'runs')",
            [],
            |r| r.get(0),
        )?;
        if !has_runs {
            return Ok(HashMap::new());
        }
        let mut stmt =
            conn.prepare("SELECT scale, MAX(bpm) FROM runs WHERE clean = 1 AND exercise = ?1 GROUP BY scale")?;
        let rows = stmt.query_map([exercise], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
        rows.collect()
    };
    if !path.exists() {
        return HashMap::new();
    }
    read().unwrap_or_else(|e| {
        log::warn!("practice history: cannot read the runs in {}: {e}", path.display());
        HashMap::new()
    })
}

/// What the history says about one scale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Practice {
    /// Time spent playing it, without pauses (see [`PAUSE`]).
    pub time: Duration,
    /// When it was last played, in ms since 1970.
    pub last_ms: Option<i64>,
}

/// Practice per scale name, from the history at `path`. Empty when there is none.
pub fn practice(path: &Path) -> HashMap<String, Practice> {
    let read = || -> rusqlite::Result<HashMap<String, Practice>> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut stmt = conn
            .prepare("SELECT scale, t_ms FROM notes WHERE velocity > 0 AND scale IS NOT NULL ORDER BY scale, t_ms")?;
        let mut out: HashMap<String, Vec<i64>> = HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (scale, t) = row?;
            out.entry(scale).or_default().push(t);
        }
        Ok(out
            .into_iter()
            .map(|(scale, times)| {
                let p = Practice {
                    time: active_time(&times),
                    last_ms: times.last().copied(),
                };
                (scale, p)
            })
            .collect())
    };
    if !path.exists() {
        return HashMap::new();
    }
    read().unwrap_or_else(|e| {
        log::warn!("practice history: cannot read {}: {e}", path.display());
        HashMap::new()
    })
}

/// Playing time from note times (ms, in order): the gaps between notes added up, except gaps
/// longer than [`PAUSE`].
pub fn active_time(times_ms: &[i64]) -> Duration {
    let pause = i64::try_from(PAUSE.as_millis()).unwrap_or(i64::MAX);
    let ms: i64 = times_ms
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => b.checked_sub(*a),
            _ => None,
        })
        .filter(|&gap| (0..=pause).contains(&gap))
        .fold(0i64, i64::saturating_add);
    Duration::from_millis(u64::try_from(ms).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onezan-progress-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("progress.sqlite")
    }

    #[test]
    fn pauses_do_not_count() {
        assert_eq!(active_time(&[]), Duration::ZERO);
        assert_eq!(active_time(&[1000]), Duration::ZERO);
        // 0.5 s and 1 s of playing, a 60 s break, then 2 s more.
        let t = [0, 500, 1500, 61_500, 62_500, 63_500];
        assert_eq!(active_time(&t), Duration::from_millis(3500));
        // Exactly the pause length still counts.
        assert_eq!(active_time(&[0, 15_000]), PAUSE);
    }

    #[test]
    fn notes_are_recorded_with_their_scale() {
        let path = temp("record");
        let rec = Recorder::start(&path).unwrap();
        let sink = rec.note_sink();
        sink(60, 100); // no scale selected: not counted
        rec.set_scale(Some("C major".into()));
        sink(60, 90);
        sink(60, 0);
        sink(62, 90);
        rec.set_scale(Some("G major".into()));
        sink(67, 80);
        drop(rec); // writes the rest, though the sink is still alive
        sink(69, 80); // after the stop: dropped, no panic
        let p = practice(&path);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p["C major"].last_ms.is_some());
        assert!(p["G major"].time.is_zero()); // one note: no time yet
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 5);
    }

    #[test]
    fn a_damaged_file_is_moved_aside() {
        let path = temp("damaged");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"this is not a database, not even close to one........").unwrap();
        let rec = Recorder::start(&path);
        assert!(rec.is_some());
        let mut aside = path.as_os_str().to_owned();
        aside.push(".broken");
        assert!(Path::new(&aside).exists());
    }

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/progress-0.1.sqlite");

    /// Writes `fixtures/progress-0.1.sqlite`. Run with `--ignored` only to make the fixture of
    /// a new version (under a new name): the old one must keep loading.
    #[test]
    #[ignore = "makes a fixture; not a check"]
    fn write_fixture() {
        let _ = std::fs::remove_file(FIXTURE);
        let conn = open_checked(Path::new(FIXTURE)).unwrap();
        // C major: three notes over 1.5 s; G major: one note; one note with no scale.
        let notes = [
            (1_700_000_000_000, 60, 90, Some("C major")),
            (1_700_000_000_500, 62, 90, Some("C major")),
            (1_700_000_000_600, 62, 0, Some("C major")),
            (1_700_000_001_500, 64, 90, Some("C major")),
            (1_700_000_100_000, 67, 80, Some("G major")),
            (1_700_000_200_000, 69, 80, None),
        ];
        let batch: Vec<_> = notes
            .iter()
            .map(|&(t, k, v, s)| (t, k, v, s.map(str::to_owned)))
            .collect();
        write(&conn, &batch).unwrap();
        // One file, no write-ahead log beside it.
        conn.pragma_update(None, "journal_mode", "DELETE").unwrap();
    }

    #[test]
    fn a_practice_history_from_version_0_1_still_loads() {
        let path = temp("fixture");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(FIXTURE, &path).unwrap();
        let p = practice(&path);
        assert_eq!(p.len(), 2, "{p:?}");
        assert_eq!(p["C major"].time, Duration::from_millis(1500));
        assert_eq!(p["C major"].last_ms, Some(1_700_000_001_500));
        assert_eq!(p["G major"].last_ms, Some(1_700_000_100_000));
        // And recording goes on in it: not moved aside, the old notes kept.
        let rec = Recorder::start(&path).unwrap();
        rec.set_scale(Some("C major".into()));
        rec.note_sink()(72, 90);
        drop(rec);
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn no_file_means_no_practice() {
        assert!(practice(&temp("none")).is_empty());
    }

    #[test]
    fn the_fastest_clean_run_is_kept_per_scale_and_exercise() {
        let path = temp("runs");
        let rec = Recorder::start(&path).unwrap();
        let run = |scale: &str, exercise: &str, bpm, clean| RunRecord {
            scale: scale.into(),
            exercise: exercise.into(),
            bpm,
            played: 29,
            notes: 29,
            spread_ms: Some(20.0),
            loudness: Some(5.0),
            clean,
        };
        rec.record_run(run("C major", "hands together/2", 80, true));
        rec.record_run(run("C major", "hands together/2", 88, true));
        rec.record_run(run("C major", "hands together/2", 96, false));
        rec.record_run(run("C major", "contrary motion/2", 60, true));
        rec.record_run(run("G major", "hands together/2", 72, true));
        drop(rec);
        let together = clean_tempos(&path, "hands together/2");
        assert_eq!(
            together.get("C major"),
            Some(&88),
            "the fastest clean one, not the faster unclean"
        );
        assert_eq!(together.get("G major"), Some(&72));
        assert_eq!(clean_tempos(&path, "contrary motion/2").get("C major"), Some(&60));
        assert!(clean_tempos(&path, "right hand/1").is_empty());
    }

    #[test]
    fn a_history_without_runs_has_no_clean_tempos() {
        let path = temp("no-runs");
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE notes (t_ms INTEGER, key INTEGER, velocity INTEGER, scale TEXT);")
            .unwrap();
        drop(conn);
        assert!(clean_tempos(&path, "hands together/2").is_empty());
    }
}
