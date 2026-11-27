//! The logger, keeping the last error it was given: when the window cannot be opened, eframe
//! only logs why (it does not return the reason), and the window's message box needs it.

use std::sync::Mutex;

static LAST: Mutex<Option<String>> = Mutex::new(None);

/// Starts logging to stderr (`RUST_LOG`, default: warnings and errors).
pub fn init() {
    let inner = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).build();
    log::set_max_level(inner.filter());
    if log::set_boxed_logger(Box::new(Keeper(inner))).is_err() {
        eprintln!("a logger was already set");
    }
}

/// The last error logged, if any.
pub fn last() -> Option<String> {
    LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

struct Keeper(env_logger::Logger);

impl log::Log for Keeper {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.0.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.level() == log::Level::Error {
            *LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(record.args().to_string());
        }
        self.0.log(record);
    }

    fn flush(&self) {
        self.0.flush();
    }
}
