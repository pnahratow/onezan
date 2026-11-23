//! The per-block event and parameter lists handed to `IAudioProcessor::process`.
//!
//! All of them are filled by the host and read by the plugin on the audio thread, one after the
//! other, so their locks are never contended. Capacity is reserved up front: no allocation while
//! processing.

use crate::util::int32 as count;
use std::sync::Mutex;
use vst3::Steinberg::Vst::{
    Event, IEventList, IEventListTrait, IParamValueQueue, IParamValueQueueTrait, IParameterChanges,
    IParameterChangesTrait, ParamID, ParamValue,
};
use vst3::Steinberg::{int32, kInvalidArgument, kResultFalse, kResultOk, tresult};
use vst3::{Class, ComWrapper};

pub const EVENT_CAP: usize = 512;
pub const PARAM_CAP: usize = 64;
const POINT_CAP: usize = 64;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct EventList {
    events: Mutex<Vec<Event>>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl EventList {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::with_capacity(EVENT_CAP)),
        }
    }
    pub fn clear(&self) {
        lock(&self.events).clear();
    }
    pub fn push(&self, e: Event) {
        let mut v = lock(&self.events);
        if v.len() < EVENT_CAP {
            v.push(e);
        }
    }
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        count(lock(&self.events).len())
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        // SAFETY: out-pointer from the plugin, or null (handled).
        let Some(out) = (unsafe { e.as_mut() }) else {
            return kInvalidArgument;
        };
        usize::try_from(index)
            .ok()
            .and_then(|i| lock(&self.events).get(i).copied())
            .map_or(kInvalidArgument, |ev| {
                *out = ev;
                kResultOk
            })
    }
    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        // Output events (e.g. MIDI out) are accepted and ignored.
        // SAFETY: in-pointer from the plugin, or null (handled).
        match unsafe { e.as_ref() } {
            Some(&ev) => {
                self.push(ev);
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
}

struct Queue {
    id: ParamID,
    points: Vec<(int32, ParamValue)>,
}

pub struct ParamQueue {
    q: Mutex<Queue>,
}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        lock(&self.q).id
    }
    unsafe fn getPointCount(&self) -> int32 {
        count(lock(&self.q).points.len())
    }
    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        // SAFETY: out-pointers from the plugin, or null (handled).
        let (Some(o), Some(v)) = (unsafe { (offset.as_mut(), value.as_mut()) }) else {
            return kInvalidArgument;
        };
        match usize::try_from(index)
            .ok()
            .and_then(|i| lock(&self.q).points.get(i).copied())
        {
            Some((at, val)) => {
                *o = at;
                *v = val;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let mut q = lock(&self.q);
        if q.points.len() >= POINT_CAP {
            return kResultFalse;
        }
        let at = count(q.points.len());
        q.points.push((offset, value));
        drop(q);
        // SAFETY: out-pointer from the plugin, or null (allowed).
        if let Some(i) = unsafe { index.as_mut() } {
            *i = at;
        }
        kResultOk
    }
}

/// A fixed pool of queues; `len` of them are in use this block.
pub struct ParamChanges {
    queues: Vec<ComWrapper<ParamQueue>>,
    len: Mutex<usize>,
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl ParamChanges {
    pub fn new() -> Self {
        let queues = (0..PARAM_CAP)
            .map(|_| {
                ComWrapper::new(ParamQueue {
                    q: Mutex::new(Queue {
                        id: 0,
                        points: Vec::with_capacity(POINT_CAP),
                    }),
                })
            })
            .collect();
        Self {
            queues,
            len: Mutex::new(0),
        }
    }

    pub fn clear(&self) {
        let mut len = lock(&self.len);
        for q in self.queues.iter().take(*len) {
            lock(&q.q).points.clear();
        }
        *len = 0;
    }

    /// Index of the queue for `id`, taking a free one if needed.
    fn queue_for(&self, id: ParamID) -> Option<usize> {
        let mut len = lock(&self.len);
        if let Some(i) = self.queues.iter().take(*len).position(|q| lock(&q.q).id == id) {
            return Some(i);
        }
        let i = *len;
        let mut q = lock(&self.queues.get(i)?.q);
        q.id = id;
        q.points.clear();
        drop(q);
        *len = i.checked_add(1)?;
        drop(len);
        Some(i)
    }

    /// Adds one point (host side).
    pub fn add(&self, id: ParamID, offset: i32, value: f64) {
        if let Some(queue) = self.queue_for(id).and_then(|i| self.queues.get(i)) {
            let mut q = lock(&queue.q);
            if q.points.len() < POINT_CAP {
                q.points.push((offset, value));
            }
        }
    }

    fn queue_ptr(&self, i: usize) -> *mut IParamValueQueue {
        self.queues
            .get(i)
            .and_then(vst3::ComWrapper::as_com_ref::<IParamValueQueue>)
            .map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        count(*lock(&self.len))
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        match usize::try_from(index) {
            Ok(i) if i < *lock(&self.len) => self.queue_ptr(i),
            _ => std::ptr::null_mut(),
        }
    }
    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        // SAFETY: in-pointer from the plugin, or null (handled).
        let Some(&id) = (unsafe { id.as_ref() }) else {
            return std::ptr::null_mut();
        };
        self.queue_for(id).map_or(std::ptr::null_mut(), |i| {
            // SAFETY: out-pointer from the plugin, or null (allowed).
            if let Some(out) = unsafe { index.as_mut() } {
                *out = count(i);
            }
            self.queue_ptr(i)
        })
    }
}
