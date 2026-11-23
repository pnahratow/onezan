//! The MIDI keyboard: opened by itself, released when unplugged, reopened when it comes back.
//!
//! midir has no hotplug notifications, so a thread polls the port list every [`POLL`]. When the
//! open port disappears, the notes it was holding are released (and the sustain pedal, mod wheel
//! and pitch bend reset), so nothing hangs. When no port is open, the first one
//! [`choose_midi_port`] picks is opened.

use crate::report::{Considered, Verdict};
use crate::timing::TimedEvent;
use anyhow::Result;
use onezan_vst3_host::MidiEvent;
use rtrb::Producer;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// How often the port list is checked. A keyboard is usable at most this long after it is plugged
/// in. (A replug faster than this, which gets the same ALSA port address back, goes unnoticed.)
pub const POLL: Duration = Duration::from_secs(1);

/// A change of the open MIDI port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiChange {
    Connected(String),
    /// The port went away; its held notes were released.
    Disconnected(String),
    /// The port is there but could not be opened; retried on every poll, reported once.
    Failed {
        port: String,
        error: String,
    },
}

/// Names of the available MIDI input ports.
///
/// # Errors
///
/// When the system MIDI client cannot be created.
pub fn midi_input_ports() -> Result<Vec<String>> {
    let input = midir::MidiInput::new("onezan")?;
    Ok(input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect())
}

/// A loopback port ("Midi Through"): it only echoes what other programs send, it is no keyboard.
fn is_loopback(port: &str) -> bool {
    port.to_lowercase().contains("through")
}

/// Picks the MIDI port: the first whose name contains `query` (case-insensitive), or, without a
/// query, the first that is not a loopback. Returns its index and a verdict for every port.
#[must_use]
pub fn choose_midi_port(ports: &[String], query: Option<&str>) -> (Option<usize>, Vec<Considered>) {
    let q = query.map(str::to_lowercase);
    let fits = |p: &String| {
        q.as_ref()
            .map_or_else(|| !is_loopback(p), |q| p.to_lowercase().contains(q))
    };
    let chosen = ports.iter().position(fits);
    let considered = ports
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (verdict, why) = match (Some(i) == chosen, &q) {
                (true, Some(q)) => (Verdict::Used, format!("matches \"{q}\"")),
                (true, None) => (Verdict::Used, "the first port that is not a loopback".to_string()),
                (false, Some(q)) if !fits(p) => (Verdict::NotUsed, format!("does not match \"{q}\"")),
                (false, None) if is_loopback(p) => (
                    Verdict::NotUsed,
                    "a loopback that echoes other programs, not a keyboard".to_string(),
                ),
                (false, _) => (
                    Verdict::NotUsed,
                    "another port was picked first; choose with --midi".to_string(),
                ),
            };
            Considered::new(p.clone(), verdict, why)
        })
        .collect();
    (chosen, considered)
}

/// The keys the keyboard holds right now (all channels merged), how hard each was struck, and
/// the sustain pedal: for display. Written by the MIDI thread, read by anyone.
pub struct Keys {
    /// Per key, the velocity it was struck with while it is held; 0 when it is up (a note-on
    /// always has a velocity of at least 1: one of 0 arrives as a note-off).
    velocity: [AtomicU8; 128],
    sustain: AtomicBool,
    waker: OnceLock<Box<dyn Fn() + Send + Sync>>,
    /// Told of every note: (key, velocity), velocity 0 for a release.
    listener: OnceLock<NoteListener>,
}

type NoteListener = Box<dyn Fn(u8, u8) + Send + Sync>;

impl Default for Keys {
    fn default() -> Self {
        Self {
            velocity: std::array::from_fn(|_| AtomicU8::new(0)),
            sustain: AtomicBool::new(false),
            waker: OnceLock::new(),
            listener: OnceLock::new(),
        }
    }
}

impl Keys {
    /// Whether `key` (a MIDI note number) is held down.
    #[must_use]
    pub fn held(&self, key: u8) -> bool {
        self.velocity(key).is_some()
    }

    /// How hard `key` was struck (1 to 127), while it is held.
    #[must_use]
    pub fn velocity(&self, key: u8) -> Option<u8> {
        let v = self.velocity.get(usize::from(key))?.load(Ordering::Relaxed);
        (v > 0).then_some(v)
    }

    /// Whether the sustain pedal is down.
    #[must_use]
    pub fn sustain(&self) -> bool {
        self.sustain.load(Ordering::Relaxed)
    }

    /// Sets what runs after every change, on the MIDI thread (for example, a GUI repaint
    /// request). Only the first call counts.
    pub fn set_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        let _ = self.waker.set(Box::new(waker));
    }

    /// Sets what is told of every note played, on the MIDI thread: (key, velocity), velocity 0
    /// for a release (for example, to record practice). Only the first call counts.
    pub fn set_note_listener(&self, listener: impl Fn(u8, u8) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    fn tell(&self, key: u8, velocity: u8) {
        if let Some(l) = self.listener.get() {
            l(key, velocity);
        }
    }

    fn update(&self, ev: MidiEvent) {
        let set = |key: u8, v: u8| {
            if let Some(slot) = self.velocity.get(usize::from(key)) {
                slot.store(v, Ordering::Relaxed);
            }
        };
        match ev {
            MidiEvent::NoteOn { key, velocity, .. } => {
                set(key, velocity.max(1));
                self.tell(key, velocity.max(1));
            }
            MidiEvent::NoteOff { key, .. } => {
                set(key, 0);
                self.tell(key, 0);
            }
            MidiEvent::ControlChange {
                controller: 64, value, ..
            } => self.sustain.store(value >= 64, Ordering::Relaxed),
            _ => return,
        }
        self.wake();
    }

    fn clear(&self) {
        for v in &self.velocity {
            v.store(0, Ordering::Relaxed);
        }
        self.sustain.store(false, Ordering::Relaxed);
        self.wake();
    }

    fn wake(&self) {
        if let Some(w) = self.waker.get() {
            w();
        }
    }
}

/// Watches the MIDI ports and keeps one open; stops when dropped.
pub struct MidiWatch {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl MidiWatch {
    /// Opens the port [`choose_midi_port`] picks for `query`, if there is one, and keeps watching
    /// every `poll`. Events go to `tx`, and what is held is kept in `keys`. `on_change` runs on the watcher thread for every later
    /// change.
    ///
    /// Returns the watcher, the result of the first attempt (`None`: no port fits yet) and the
    /// verdict on every port found then.
    ///
    /// # Errors
    ///
    /// When the system MIDI client cannot be created.
    pub fn start(
        query: Option<String>,
        poll: Duration,
        tx: Producer<TimedEvent>,
        keys: Arc<Keys>,
        mut on_change: impl FnMut(MidiChange) + Send + 'static,
    ) -> Result<(Self, Option<MidiChange>, Vec<Considered>)> {
        let mut watcher = Watcher {
            list: midir::MidiInput::new("onezan")?,
            query,
            feed: Arc::new(Mutex::new(Feed::new(tx, keys))),
            open: None,
            last_error: None,
        };
        let (first, considered) = watcher.step();
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("onezan-midi-watch".into())
            .spawn(move || {
                while stopped.recv_timeout(poll) == Err(RecvTimeoutError::Timeout) {
                    if let (Some(change), _) = watcher.step() {
                        on_change(change);
                    }
                }
            })?;
        Ok((
            Self {
                stop: Some(stop),
                thread: Some(thread),
            },
            first,
            considered,
        ))
    }
}

impl Drop for MidiWatch {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Open {
    id: String,
    name: String,
    _conn: midir::MidiInputConnection<()>,
}

struct Watcher {
    /// Only lists ports; each connection gets a client of its own (midir consumes it).
    list: midir::MidiInput,
    query: Option<String>,
    feed: Arc<Mutex<Feed>>,
    open: Option<Open>,
    /// The last failure reported, so a port that keeps failing is reported once.
    last_error: Option<(String, String)>,
}

impl Watcher {
    /// One poll: notices a vanished port, or opens one. Also returns the verdict on every port.
    fn step(&mut self) -> (Option<MidiChange>, Vec<Considered>) {
        let ports: Vec<(String, String)> = self
            .list
            .ports()
            .iter()
            .filter_map(|p| self.list.port_name(p).ok().map(|n| (p.id(), n)))
            .collect();
        if let Some(open) = self.open.take() {
            if ports.iter().any(|(id, _)| *id == open.id) {
                self.open = Some(open);
                return (None, Vec::new());
            }
            let name = open.name.clone();
            // Closes the connection before the release, so no late event follows it.
            drop(open);
            lock(&self.feed).release_all();
            return (Some(MidiChange::Disconnected(name)), Vec::new());
        }
        let names: Vec<String> = ports.iter().map(|(_, n)| n.clone()).collect();
        let (chosen, considered) = choose_midi_port(&names, self.query.as_deref());
        let Some((id, name)) = chosen.and_then(|i| ports.get(i)) else {
            return (None, considered);
        };
        let change = match self.connect(id, name) {
            Ok(open) => {
                self.open = Some(open);
                self.last_error = None;
                Some(MidiChange::Connected(name.clone()))
            }
            Err(error) => {
                let failure = (name.clone(), error);
                let repeated = self.last_error.as_ref() == Some(&failure);
                self.last_error = Some(failure.clone());
                (!repeated).then_some(MidiChange::Failed {
                    port: failure.0,
                    error: failure.1,
                })
            }
        };
        (change, considered)
    }

    fn connect(&self, id: &str, name: &str) -> Result<Open, String> {
        let mut input = midir::MidiInput::new("onezan").map_err(|e| e.to_string())?;
        input.ignore(midir::Ignore::All);
        let port = input.find_port_by_id(id.to_string()).ok_or("the port went away")?;
        let feed = Arc::clone(&self.feed);
        let conn = input
            .connect(
                &port,
                "onezan-in",
                move |_stamp, bytes, ()| {
                    // Stamped on arrival; midir's own stamp is on another clock (and per platform).
                    if let Some(ev) = MidiEvent::parse(bytes) {
                        lock(&feed).send(ev);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(Open {
            id: id.to_string(),
            name: name.to_string(),
            _conn: conn,
        })
    }
}

/// The MIDI thread and the watcher both write; a panic in one must not silence the other.
fn lock(feed: &Mutex<Feed>) -> std::sync::MutexGuard<'_, Feed> {
    feed.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The queue to the audio thread, and what the keyboard holds, so it can be released.
struct Feed {
    tx: Producer<TimedEvent>,
    /// Per channel, a bit per held key.
    held: [u128; 16],
    /// A bit per channel that saw any event.
    used: u16,
    keys: Arc<Keys>,
}

impl Feed {
    const fn new(tx: Producer<TimedEvent>, keys: Arc<Keys>) -> Self {
        Self {
            tx,
            held: [0; 16],
            used: 0,
            keys,
        }
    }

    fn send(&mut self, ev: MidiEvent) {
        match ev {
            MidiEvent::NoteOn { channel, key, .. } => {
                if let Some(keys) = self.held.get_mut(usize::from(channel)) {
                    *keys |= bit128(key);
                }
            }
            MidiEvent::NoteOff { channel, key, .. } => {
                if let Some(keys) = self.held.get_mut(usize::from(channel)) {
                    *keys &= !bit128(key);
                }
            }
            _ => {}
        }
        self.used |= 1u16.checked_shl(u32::from(channel_of(ev))).unwrap_or(0);
        // A full queue drops the event; the audio thread is not keeping up anyway.
        let _ = self.tx.push(TimedEvent::now(ev));
        self.keys.update(ev);
    }

    /// Note-offs for every held key; sustain, mod wheel and pitch bend back to rest.
    fn release_all(&mut self) {
        for (channel, keys) in (0u8..16).zip(self.held) {
            for key in (0u8..128).filter(|&k| keys & bit128(k) != 0) {
                let _ = self.tx.push(TimedEvent::now(MidiEvent::NoteOff {
                    channel,
                    key,
                    velocity: 64,
                }));
            }
            if self.used & 1u16.checked_shl(u32::from(channel)).unwrap_or(0) == 0 {
                continue;
            }
            for controller in [64, 1] {
                let _ = self.tx.push(TimedEvent::now(MidiEvent::ControlChange {
                    channel,
                    controller,
                    value: 0,
                }));
            }
            let _ = self
                .tx
                .push(TimedEvent::now(MidiEvent::PitchBend { channel, value: 8192 }));
        }
        self.held = [0; 16];
        self.used = 0;
        self.keys.clear();
    }
}

fn bit128(key: u8) -> u128 {
    1u128.checked_shl(u32::from(key)).unwrap_or(0)
}

const fn channel_of(ev: MidiEvent) -> u8 {
    match ev {
        MidiEvent::NoteOn { channel, .. }
        | MidiEvent::NoteOff { channel, .. }
        | MidiEvent::PolyPressure { channel, .. }
        | MidiEvent::ControlChange { channel, .. }
        | MidiEvent::ChannelPressure { channel, .. }
        | MidiEvent::PitchBend { channel, .. } => channel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut rtrb::Consumer<TimedEvent>) -> Vec<MidiEvent> {
        std::iter::from_fn(|| rx.pop().ok().map(|e| e.event)).collect()
    }

    #[test]
    fn release_all_frees_held_notes_and_controllers() {
        let (tx, mut rx) = crate::midi_queue();
        let keys = Arc::new(Keys::default());
        let mut feed = Feed::new(tx, Arc::clone(&keys));
        let on = |key| MidiEvent::NoteOn {
            channel: 2,
            key,
            velocity: 100,
        };
        feed.send(on(60));
        feed.send(on(127));
        feed.send(on(64));
        feed.send(MidiEvent::NoteOff {
            channel: 2,
            key: 64,
            velocity: 64,
        });
        drain(&mut rx);
        assert!(keys.held(60) && keys.held(127) && !keys.held(64));
        assert_eq!(keys.velocity(60), Some(100));
        assert_eq!(keys.velocity(64), None);
        feed.release_all();
        assert!(!keys.held(60) && !keys.held(127));
        let off = |key| MidiEvent::NoteOff {
            channel: 2,
            key,
            velocity: 64,
        };
        let cc = |controller| MidiEvent::ControlChange {
            channel: 2,
            controller,
            value: 0,
        };
        assert_eq!(
            drain(&mut rx),
            [
                off(60),
                off(127),
                cc(64),
                cc(1),
                MidiEvent::PitchBend {
                    channel: 2,
                    value: 8192
                }
            ]
        );
        // Nothing is held any more.
        feed.release_all();
        assert_eq!(drain(&mut rx), []);
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn midi_skips_loopback_without_query() {
        let ports = names(&["Midi Through Port-0", "EasyKeys49 MIDI 1", "Other Keys"]);
        let (chosen, c) = choose_midi_port(&ports, None);
        assert_eq!(chosen, Some(1));
        assert_eq!(
            c.iter().map(|c| c.verdict).collect::<Vec<_>>(),
            [Verdict::NotUsed, Verdict::Used, Verdict::NotUsed]
        );
        assert!(c[0].why.contains("loopback"));
        assert!(c[2].why.contains("--midi"));
    }

    #[test]
    fn midi_query_matches_case_insensitively() {
        let ports = names(&["Midi Through Port-0", "EasyKeys49 MIDI 1"]);
        let (chosen, c) = choose_midi_port(&ports, Some("THROUGH"));
        assert_eq!(chosen, Some(0));
        assert!(c[1].why.contains("does not match"));
    }

    #[test]
    fn midi_none_when_only_loopback() {
        let (chosen, c) = choose_midi_port(&names(&["Midi Through Port-0"]), None);
        assert_eq!(chosen, None);
        assert_eq!(c[0].verdict, Verdict::NotUsed);
    }
}
