//! The plugin's editor inside the window's middle area.
//!
//! Linux: the editor needs an X11 parent, so the window runs on X11 (XWayland under a Wayland
//! desktop, see `gui::run`). A child window of ours is placed over the middle area and the editor
//! is attached to it; it is moved and resized with the layout.
//!
//! Windows: the same with a child `HWND` of the main window.

use onezan_vst3_host::Editor;

/// A rectangle in physical pixels, relative to the window's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// The editor, embedded in a child window of ours.
pub struct Embedded {
    // Dropped first: the editor is removed while its parent window still exists.
    editor: Editor,
    child: ChildWindow,
    /// The size the editor has, in pixels.
    pub size: (u32, u32),
    /// Where the child window is.
    placed: Option<PixelRect>,
    /// The size of the area last fitted into.
    area: Option<(u32, u32)>,
    hidden: bool,
}

impl Embedded {
    /// Makes a child window of `parent` at `at` and attaches `editor` to it.
    pub fn new(mut editor: Editor, parent: u64, at: PixelRect) -> Result<Self, String> {
        let size = editor.size().unwrap_or((at.w, at.h));
        let child = ChildWindow::new(
            parent,
            PixelRect {
                w: size.0,
                h: size.1,
                ..at
            },
        )?;
        // SAFETY: `child` is a live X11 window of ours; it outlives the editor (dropped after it,
        // field order).
        unsafe { editor.attach(child.handle()) }.map_err(|e| e.to_string())?;
        Ok(Self {
            editor,
            child,
            size,
            placed: None,
            area: None,
            hidden: false,
        })
    }

    pub const fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Takes a size the editor asked for. Returns whether it changed.
    ///
    /// The size it then reports wins over the one it asked for: Surge XT on Windows (JUCE) asks
    /// for its old size right after a resize, but stays at the new one.
    pub fn follow_requests(&mut self) -> bool {
        match self
            .editor
            .requested_size()
            .map(|asked| self.editor.size().unwrap_or(asked))
        {
            Some(s) if s != self.size => {
                self.size = s;
                self.placed = None;
                true
            }
            _ => false,
        }
    }

    /// Hides the editor (the falling notes take its place) or shows it again.
    pub fn set_hidden(&mut self, hidden: bool) {
        if self.hidden != hidden {
            self.hidden = hidden;
            self.child.set_visible(!hidden);
        }
    }

    /// Fits the editor into `area`: a resizable editor is asked to fill it when the area changes
    /// (it may keep its own proportions); any editor sits at the top, centred across, and is cut
    /// off where it does not fit.
    pub fn place(&mut self, area: PixelRect) {
        if self.area != Some((area.w, area.h)) {
            self.area = Some((area.w, area.h));
            if let Some(s) = self.editor.resize(area.w, area.h) {
                self.size = s;
            }
        }
        let (w, h) = (self.size.0.min(area.w), self.size.1.min(area.h));
        let centre = |free: u32| i32::try_from(free / 2).unwrap_or(0);
        let at = PixelRect {
            x: area.x.saturating_add(centre(area.w.saturating_sub(w))),
            y: area.y,
            w,
            h,
        };
        if self.placed != Some(at) {
            self.child.set_geometry(at);
            self.placed = Some(at);
        }
    }
}

// ---- the child window ------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub struct ChildWindow {
    conn: x11rb::rust_connection::RustConnection,
    window: u32,
}

#[cfg(target_os = "linux")]
impl ChildWindow {
    fn new(parent: u64, at: PixelRect) -> Result<Self, String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, WindowClass};
        let parent = u32::try_from(parent).map_err(|_| "not an X11 window id".to_string())?;
        let (conn, _) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
        let window = conn.generate_id().map_err(|e| e.to_string())?;
        let (w, h) = (to_u16(at.w), to_u16(at.h));
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            parent,
            to_i16(at.x),
            to_i16(at.y),
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().background_pixel(0),
        )
        .map_err(|e| e.to_string())?;
        conn.map_window(window).map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        Ok(Self { conn, window })
    }

    /// The window as the editor's parent: its X11 id.
    fn handle(&self) -> *mut std::ffi::c_void {
        std::ptr::without_provenance_mut(usize::try_from(self.window).unwrap_or(0))
    }

    fn set_geometry(&self, at: PixelRect) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt};
        let aux = ConfigureWindowAux::new()
            .x(at.x)
            .y(at.y)
            .width(at.w.max(1))
            .height(at.h.max(1));
        let _ = self.conn.configure_window(self.window, &aux);
        let _ = self.conn.flush();
    }

    fn set_visible(&self, visible: bool) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let _ = if visible {
            self.conn.map_window(self.window)
        } else {
            self.conn.unmap_window(self.window)
        };
        let _ = self.conn.flush();
    }
}

#[cfg(target_os = "linux")]
impl Drop for ChildWindow {
    fn drop(&mut self) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
    }
}

#[cfg(target_os = "linux")]
fn to_u16(n: u32) -> u16 {
    u16::try_from(n.max(1)).unwrap_or(u16::MAX)
}

#[cfg(target_os = "linux")]
fn to_i16(n: i32) -> i16 {
    i16::try_from(n).unwrap_or(if n < 0 { i16::MIN } else { i16::MAX })
}

// Windows: a child window of ours inside the main window (`WS_CHILD`); the editor makes its own
// child window in it. The plugin's timers and input arrive as window messages, which winit's
// loop dispatches on this (the UI) thread. winit gives its windows `WS_CLIPCHILDREN`, so our
// OpenGL drawing does not paint over the editor.

#[cfg(target_os = "windows")]
pub struct ChildWindow {
    hwnd: windows_sys::Win32::Foundation::HWND,
}

#[cfg(target_os = "windows")]
impl ChildWindow {
    fn new(parent: u64, at: PixelRect) -> Result<Self, String> {
        use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
            WS_VISIBLE,
        };
        let parent: windows_sys::Win32::Foundation::HWND =
            std::ptr::with_exposed_provenance_mut(usize::try_from(parent).map_err(|_| "not a window handle")?);
        let class: Vec<u16> = "onezan-plugin-editor\0".encode_utf16().collect();
        // SAFETY: plain Win32 calls on the UI thread; `class` outlives them (a class registered
        // twice fails harmlessly, the first one stays).
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: instance,
                hbrBackground: GetStockObject(BLACK_BRUSH),
                lpszClassName: class.as_ptr(),
                // SAFETY: all-zero is valid for the remaining fields (no icon, cursor, menu).
                ..std::mem::zeroed()
            };
            RegisterClassW(&raw const wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                at.x,
                at.y,
                to_i32(at.w),
                to_i32(at.h),
                parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return Err(format!(
                    "cannot make a child window: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self { hwnd })
        }
    }

    /// The window as the editor's parent: its `HWND`.
    const fn handle(&self) -> *mut std::ffi::c_void {
        self.hwnd
    }

    fn set_geometry(&self, at: PixelRect) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
        // SAFETY: our live window, UI thread.
        unsafe {
            SetWindowPos(
                self.hwnd,
                std::ptr::null_mut(),
                at.x,
                at.y,
                to_i32(at.w),
                to_i32(at.h),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn set_visible(&self, visible: bool) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow};
        // SAFETY: our live window, UI thread.
        unsafe {
            ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for ChildWindow {
    fn drop(&mut self) {
        // SAFETY: our live window, UI thread; the editor was removed from it first.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd) };
    }
}

#[cfg(target_os = "windows")]
fn to_i32(n: u32) -> i32 {
    i32::try_from(n.max(1)).unwrap_or(i32::MAX)
}

/// Others: not done; [`ChildWindow::new`] always fails.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub struct ChildWindow;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
#[expect(
    clippy::unused_self,
    clippy::missing_const_for_fn,
    reason = "a stub: no embedding on this platform"
)]
impl ChildWindow {
    fn new(_parent: u64, _at: PixelRect) -> Result<Self, String> {
        Err("embedding the plugin's editor is not done on this platform".into())
    }

    fn handle(&self) -> *mut std::ffi::c_void {
        std::ptr::null_mut()
    }

    fn set_geometry(&self, _at: PixelRect) {}

    fn set_visible(&self, _visible: bool) {}
}
