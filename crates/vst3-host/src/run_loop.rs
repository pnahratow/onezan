//! Linux: the host's `IRunLoop` for a plugin's editor.
//!
//! A Linux editor has no system message loop to ride on. It asks its frame for `IRunLoop` and
//! registers timers and file descriptors (typically its own X11 connection, and timer
//! descriptors); the host calls them on the UI thread. The app calls [`RunLoop::service`] often
//! (every few milliseconds while an editor is open): it calls the handlers whose descriptor is
//! readable and the timers that are due, and costs a `poll` without waiting otherwise.

use crate::editor::Frame;
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use vst3::ComPtr;
use vst3::Steinberg::Linux::{
    FileDescriptor, IEventHandler, IEventHandlerTrait, IRunLoopTrait, ITimerHandler, ITimerHandlerTrait, TimerInterval,
};
use vst3::Steinberg::{kInvalidArgument, kResultFalse, kResultTrue, tresult};

/// Timers no faster than this (some plugins ask for 0 ms).
const MIN_TIMER: Duration = Duration::from_millis(1);

struct Timer {
    handler: ComPtr<ITimerHandler>,
    every: Duration,
    next: Instant,
}

#[derive(Default)]
pub struct RunLoop {
    handlers: RefCell<Vec<(FileDescriptor, ComPtr<IEventHandler>)>>,
    timers: RefCell<Vec<Timer>>,
    closed: Cell<bool>,
}

impl RunLoop {
    /// Calls the handlers whose descriptor is readable and the timers that are due. UI thread.
    pub fn service(&self) {
        if self.closed.get() {
            return;
        }
        // Handlers are called without a borrow held: they may register or unregister.
        let handlers: Vec<(FileDescriptor, ComPtr<IEventHandler>)> = self.handlers.borrow().clone();
        for (fd, handler) in handlers {
            if readable(fd) && self.has_handler(&handler) {
                // SAFETY: the plugin's live handler (we hold a reference), on the UI thread.
                unsafe { handler.onFDIsSet(fd) };
            }
        }
        let now = Instant::now();
        let due: Vec<ComPtr<ITimerHandler>> = self
            .timers
            .borrow_mut()
            .iter_mut()
            .filter(|t| t.next <= now)
            .map(|t| {
                t.next = now.checked_add(t.every).unwrap_or(now);
                t.handler.clone()
            })
            .collect();
        for handler in due {
            if self.has_timer(&handler) {
                // SAFETY: as above.
                unsafe { handler.onTimer() };
            }
        }
    }

    fn has_handler(&self, h: &ComPtr<IEventHandler>) -> bool {
        self.handlers.borrow().iter().any(|(_, x)| x.as_ptr() == h.as_ptr())
    }

    fn has_timer(&self, h: &ComPtr<ITimerHandler>) -> bool {
        self.timers.borrow().iter().any(|t| t.handler.as_ptr() == h.as_ptr())
    }

    /// The view is gone: drop what it left registered, and take nothing more. UI thread.
    pub fn close(&self) {
        self.closed.set(true);
        let left = self.handlers.borrow().len().saturating_add(self.timers.borrow().len());
        if left > 0 {
            log::debug!("vst3 editor: {left} run-loop registrations left at teardown; dropped");
        }
        // Moved out first: releasing a handler may call back in.
        let handlers = std::mem::take(&mut *self.handlers.borrow_mut());
        let timers = std::mem::take(&mut *self.timers.borrow_mut());
        drop((handlers, timers));
    }
}

/// Whether `fd` has something to read (or an error to report) right now.
fn readable(fd: FileDescriptor) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd; no wait.
    let n = unsafe { libc::poll(&raw mut p, 1, 0) };
    n > 0 && p.revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP) != 0
}

impl IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(&self, handler: *mut IEventHandler, fd: FileDescriptor) -> tresult {
        let rl = &self.run_loop;
        // SAFETY: the plugin's live handler, or null (refused); `to_com_ptr` takes our reference.
        let Some(h) = (unsafe { vst3::ComRef::from_raw(handler) }).map(|r| r.to_com_ptr()) else {
            return kInvalidArgument;
        };
        if rl.closed.get() {
            return kResultFalse;
        }
        rl.handlers.borrow_mut().push((fd, h));
        log::debug!("vst3 editor: IRunLoop handler for fd {fd}");
        kResultTrue
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        let rl = &self.run_loop;
        let gone: Vec<_> = {
            let mut hs = rl.handlers.borrow_mut();
            let (gone, keep) = std::mem::take(&mut *hs)
                .into_iter()
                .partition(|(_, h)| h.as_ptr() == handler);
            *hs = keep;
            gone
        };
        // Released outside the borrow: the last release may call back in.
        let found = !gone.is_empty();
        drop(gone);
        if found { kResultTrue } else { kInvalidArgument }
    }

    unsafe fn registerTimer(&self, handler: *mut ITimerHandler, milliseconds: TimerInterval) -> tresult {
        let rl = &self.run_loop;
        // SAFETY: as in `registerEventHandler`.
        let Some(h) = (unsafe { vst3::ComRef::from_raw(handler) }).map(|r| r.to_com_ptr()) else {
            return kInvalidArgument;
        };
        if rl.closed.get() {
            return kResultFalse;
        }
        let every = Duration::from_millis(milliseconds).max(MIN_TIMER);
        let now = Instant::now();
        rl.timers.borrow_mut().push(Timer {
            handler: h,
            every,
            next: now.checked_add(every).unwrap_or(now),
        });
        log::debug!("vst3 editor: IRunLoop timer every {milliseconds} ms");
        kResultTrue
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        let rl = &self.run_loop;
        let gone: Vec<_> = {
            let mut ts = rl.timers.borrow_mut();
            let (gone, keep) = std::mem::take(&mut *ts)
                .into_iter()
                .partition(|t| t.handler.as_ptr() == handler);
            *ts = keep;
            gone
        };
        let found = !gone.is_empty();
        drop(gone);
        if found { kResultTrue } else { kInvalidArgument }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use vst3::{Class, ComWrapper};

    struct Counter(AtomicU32);

    impl Class for Counter {
        type Interfaces = (ITimerHandler, IEventHandler);
    }

    impl ITimerHandlerTrait for Counter {
        unsafe fn onTimer(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl IEventHandlerTrait for Counter {
        unsafe fn onFDIsSet(&self, fd: FileDescriptor) {
            // Read what woke us, as a plugin reads its X11 connection.
            let mut b = [0u8; 8];
            // SAFETY: a valid descriptor of the test's socket; `b` is writable.
            unsafe { libc::read(fd, b.as_mut_ptr().cast(), b.len()) };
            self.0.fetch_add(1000, Ordering::Relaxed);
        }
    }

    /// Services the loop every millisecond, as the app does, until `done` or 2 s.
    fn pump(rl: &RunLoop, done: impl Fn() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            rl.service();
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn timers_and_descriptors_run_and_stop_when_unregistered() {
        let frame = ComWrapper::new(Frame {
            requested: Cell::new(None),
            waker: Arc::default(),
            run_loop: RunLoop::default(),
        });
        let counter = ComWrapper::new(Counter(AtomicU32::new(0)));
        let timer = counter.as_com_ref::<ITimerHandler>().unwrap().as_ptr();
        let events = counter.as_com_ref::<IEventHandler>().unwrap().as_ptr();
        let (mut tx, rx) = UnixStream::pair().unwrap();
        let count = || counter.0.load(Ordering::Relaxed);
        // SAFETY: live handlers of ours, on this thread.
        unsafe {
            assert_eq!(frame.registerTimer(timer, 5), kResultTrue);
            assert_eq!(frame.registerEventHandler(events, rx.as_raw_fd()), kResultTrue);
        }
        assert!(pump(&frame.run_loop, || count() >= 3), "the timer fires");
        assert!(count() < 1000, "nothing to read yet");
        tx.write_all(b"x").unwrap();
        assert!(
            pump(&frame.run_loop, || count() >= 1000),
            "the descriptor's handler runs"
        );
        // SAFETY: as above.
        unsafe {
            assert_eq!(frame.unregisterTimer(timer), kResultTrue);
            assert_eq!(frame.unregisterTimer(timer), kInvalidArgument, "only once");
        }
        let before = count();
        std::thread::sleep(Duration::from_millis(30));
        frame.run_loop.service();
        assert_eq!(count(), before, "an unregistered timer stays quiet");
        frame.run_loop.close();
        // SAFETY: as above.
        unsafe { assert_eq!(frame.registerTimer(timer, 5), kResultFalse, "nothing after close") };
    }
}
