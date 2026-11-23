//! Windows: the app is a GUI program (`windows_subsystem = "windows"` in `main.rs`), so starting
//! it opens no console window. Text output then goes nowhere by default; this module connects it
//! to a console when there is a use for one:
//!
//! - started from a terminal (cmd, PowerShell): [`attach`] writes into that terminal;
//! - a command-line command without a terminal (`onezan play` from a shortcut): [`ensure`]
//!   opens a console of its own.
//!
//! Output that is redirected (`onezan list > plugins.txt`) is left alone. Elsewhere nothing here
//! does anything.
//!
//! A GUI program does not make the terminal wait: the prompt comes back at once and the output
//! appears below it, and the terminal also reads the keyboard (so `play`'s "press Enter" competes
//! with it; Ctrl+C works).

/// Connects text output to the terminal the app was started from, if any. Call first thing.
#[cfg(windows)]
pub fn attach() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call; fails harmlessly without a parent console.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
        connect_std_handles();
    }
}

/// Makes sure output goes somewhere (a command-line command): when it goes nowhere (no terminal,
/// not redirected), a console of its own is opened. (Not "no console window": under Wine a
/// terminal is a console without one, and `AllocConsole` would take the output away from it.)
#[cfg(windows)]
pub fn ensure() {
    use windows_sys::Win32::System::Console::{AllocConsole, GetStdHandle, STD_OUTPUT_HANDLE};
    // SAFETY: plain Win32 calls.
    if !valid(unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }) && unsafe { AllocConsole() } != 0 {
        connect_std_handles();
    }
}

#[cfg(windows)]
fn valid(h: windows_sys::Win32::Foundation::HANDLE) -> bool {
    !h.is_null() && h != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
}

/// Points standard input, output and error at the console, where they are not set already
/// (redirected to a file or pipe, they are, and stay so). A GUI program starts without them.
#[cfg(windows)]
fn connect_std_handles() {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    let open = |name: &str| {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `wide` is a NUL-terminated name that outlives the call.
        unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        }
    };
    let streams: [(STD_HANDLE, &str); 3] = [
        (STD_INPUT_HANDLE, "CONIN$"),
        (STD_OUTPUT_HANDLE, "CONOUT$"),
        (STD_ERROR_HANDLE, "CONOUT$"),
    ];
    for (which, device) in streams {
        // SAFETY: plain Win32 calls; the handle opened stays open for the process's life.
        unsafe {
            if !valid(GetStdHandle(which)) {
                let h = open(device);
                if h != INVALID_HANDLE_VALUE {
                    SetStdHandle(which, h);
                }
            }
        }
    }
}

#[cfg(not(windows))]
pub const fn attach() {}

#[cfg(not(windows))]
pub const fn ensure() {}
