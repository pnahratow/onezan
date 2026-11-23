//! The plugin's own editor (`IPlugView`), embedded in a window the app provides.
//!
//! The app gives a native parent window (an X11 window id on Linux, an `HWND` on Windows) and
//! sizes and places it; the plugin draws into it. Main thread only, like the controller.
//!
//! Linux has no system message loop a plugin could use for its timers and its X11 connection, so
//! the view asks its frame for the host's `IRunLoop` ([`crate::run_loop`]): the app calls
//! [`Editor::idle`] every few milliseconds while the editor is open.

use crate::Error;
use crate::util::{int32, ok};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::{Arc, OnceLock};
use vst3::Steinberg::{FIDString, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect, tresult};
use vst3::Steinberg::{kInvalidArgument, kResultTrue};
use vst3::{Class, ComPtr, ComWrapper};

/// The window type this platform's editors are embedded in; `None` where there is none yet.
#[cfg(target_os = "linux")]
const PLATFORM: Option<FIDString> = Some(vst3::Steinberg::kPlatformTypeX11EmbedWindowID);
#[cfg(target_os = "windows")]
const PLATFORM: Option<FIDString> = Some(vst3::Steinberg::kPlatformTypeHWND);
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
const PLATFORM: Option<FIDString> = None;

/// What wakes the app's UI thread when the editor asked for a new size.
pub type Waker = OnceLock<Box<dyn Fn() + Send + Sync>>;

pub fn wake(waker: &Waker) {
    if let Some(w) = waker.get() {
        w();
    }
}

/// The `IPlugFrame` the view talks to: size requests, and (Linux) the run loop.
pub struct Frame {
    /// The last size the view asked for with `resizeView`, not yet taken by the app.
    pub requested: Cell<Option<(u32, u32)>>,
    pub waker: Arc<Waker>,
    #[cfg(target_os = "linux")]
    pub run_loop: crate::run_loop::RunLoop,
}

impl Class for Frame {
    #[cfg(target_os = "linux")]
    type Interfaces = (IPlugFrame, vst3::Steinberg::Linux::IRunLoop);
    #[cfg(not(target_os = "linux"))]
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        // SAFETY: in-pointers from the plugin, valid for this call, or null (handled).
        let (Some(view), Some(r)) = (unsafe { vst3::ComRef::from_raw(view) }, unsafe { new_size.as_mut() }) else {
            return kInvalidArgument;
        };
        self.requested.set(Some(rect_size(r)));
        // The SDK has the host resize, then call `onSize`. The app resizes the parent window at
        // its next frame (woken here); the view takes the size now.
        // SAFETY: the plugin's live view, on its UI thread; `r` outlives the call.
        unsafe { view.onSize(r) };
        wake(&self.waker);
        kResultTrue
    }
}

/// An open editor. Drop it before the parent window goes and before its [`crate::Plugin`].
pub struct Editor {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<Frame>,
    waker: Arc<Waker>,
    attached: bool,
    /// Keeps the plugin from being torn down under its editor.
    _token: Arc<()>,
}

/// Width and height of a view rect, clamped to sane sizes.
fn rect_size(r: &ViewRect) -> (u32, u32) {
    let side = |a: i32, b: i32| u32::try_from(b.saturating_sub(a).clamp(1, 16_384)).unwrap_or(1);
    (side(r.left, r.right), side(r.top, r.bottom))
}

fn rect(w: u32, h: u32) -> ViewRect {
    let side = |n: u32| int32(usize::try_from(n).unwrap_or(usize::MAX));
    ViewRect {
        left: 0,
        top: 0,
        right: side(w),
        bottom: side(h),
    }
}

impl Editor {
    /// The view of `controller`, if it has one for this platform's windows.
    ///
    /// # Errors
    ///
    /// When there is no view, or none for this platform's windows.
    pub(crate) fn new(
        controller: &ComPtr<vst3::Steinberg::Vst::IEditController>,
        token: Arc<()>,
    ) -> Result<Self, Error> {
        use vst3::Steinberg::Vst::IEditControllerTrait;
        let platform = PLATFORM.ok_or_else(|| Error::Editor("no editor support on this platform".into()))?;
        // SAFETY: valid controller, main thread; the name is a static NUL-terminated string.
        let raw = unsafe { controller.createView(c"editor".as_ptr()) };
        // SAFETY: a non-null view comes with one reference, now ours.
        let view = unsafe { ComPtr::from_raw(raw) }.ok_or_else(|| Error::Editor("the plugin has no editor".into()))?;
        // SAFETY: valid view; `platform` is a static NUL-terminated string.
        if unsafe { view.isPlatformTypeSupported(platform) } != kResultTrue {
            return Err(Error::Editor(
                "the editor does not support this platform's windows".into(),
            ));
        }
        let waker = Arc::new(Waker::new());
        let frame = ComWrapper::new(Frame {
            requested: Cell::new(None),
            waker: Arc::clone(&waker),
            #[cfg(target_os = "linux")]
            run_loop: crate::run_loop::RunLoop::default(),
        });
        if let Some(f) = frame.as_com_ref::<IPlugFrame>() {
            // SAFETY: valid view; the frame outlives the view's use of it (cleared in `drop`).
            unsafe { view.setFrame(f.as_ptr()) };
        }
        Ok(Self {
            view,
            frame,
            waker,
            attached: false,
            _token: token,
        })
    }

    /// Sets what runs when the editor asked for a new size (see [`Editor::requested_size`]).
    /// Only the first call counts.
    pub fn set_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        let _ = self.waker.set(Box::new(waker));
    }

    /// The size the editor wants, in pixels.
    #[must_use]
    pub fn size(&self) -> Option<(u32, u32)> {
        let mut r = rect(0, 0);
        // SAFETY: valid view; `r` is writable.
        (ok(unsafe { self.view.getSize(&raw mut r) }) && r.right > r.left && r.bottom > r.top).then(|| rect_size(&r))
    }

    /// Whether the editor can take other sizes than its own.
    #[must_use]
    pub fn can_resize(&self) -> bool {
        // SAFETY: valid view.
        unsafe { self.view.canResize() == kResultTrue }
    }

    /// Embeds the editor in `parent`.
    ///
    /// # Safety
    ///
    /// `parent` is a live native window of this platform's kind (an X11 window id on Linux, an
    /// `HWND` on Windows) that stays alive until this editor is dropped.
    ///
    /// # Errors
    ///
    /// When the editor refuses the window.
    pub unsafe fn attach(&mut self, parent: *mut c_void) -> Result<(), Error> {
        let platform = PLATFORM.ok_or_else(|| Error::Editor("no editor support on this platform".into()))?;
        // SAFETY: valid view; the caller vouches for `parent`.
        if !ok(unsafe { self.view.attached(parent, platform) }) {
            return Err(Error::Editor("the editor refused the window".into()));
        }
        self.attached = true;
        Ok(())
    }

    /// Asks a resizable editor to take `w`×`h`, or the nearest size it allows. Returns the size
    /// it took.
    #[must_use]
    pub fn resize(&self, w: u32, h: u32) -> Option<(u32, u32)> {
        if !self.can_resize() {
            return None;
        }
        let mut r = rect(w, h);
        // SAFETY: valid view; `r` is writable.
        unsafe { self.view.checkSizeConstraint(&raw mut r) };
        // SAFETY: valid view; `r` outlives the call.
        ok(unsafe { self.view.onSize(&raw mut r) }).then(|| rect_size(&r))
    }

    /// A size the editor asked for itself since the last call.
    #[must_use]
    pub fn requested_size(&self) -> Option<(u32, u32)> {
        self.frame.requested.take()
    }

    /// Runs what the editor's run loop has due (Linux: its timers and descriptors). Call on the
    /// UI thread every few milliseconds while the editor is open; cheap when nothing is due.
    #[cfg_attr(
        not(target_os = "linux"),
        expect(clippy::missing_const_for_fn, reason = "does something on Linux only")
    )]
    pub fn idle(&self) {
        #[cfg(target_os = "linux")]
        self.frame.run_loop.service();
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        // SAFETY: valid view, main thread; `removed` while the parent window still exists (the
        // caller's contract), then our frame is taken off before it goes.
        unsafe {
            if self.attached {
                self.view.removed();
            }
            self.view.setFrame(std::ptr::null_mut());
        }
        // After `removed`: what the view left registered is dropped.
        #[cfg(target_os = "linux")]
        self.frame.run_loop.close();
    }
}
