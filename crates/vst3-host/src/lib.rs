//! A small VST3 host: find instrument plugins, load one, feed it MIDI and pull stereo audio out.
//!
//! Built directly on the `vst3` crate's generated bindings. Only what a "play a keyboard through
//! a plugin" app needs is implemented: one instrument, note/controller input, a stereo main output.
//! The plugin's editor can be embedded in a window of the app ([`Editor`]). State saving comes
//! later.
//!
//! Threading: [`Plugin`] lives on the main thread. [`Plugin::start`] hands out a [`Processor`],
//! which is `Send` and is moved to the audio thread. Drop the processor (stop the audio stream)
//! before calling [`Plugin::stop`] or dropping the plugin.

mod editor;
mod events;
mod host;
mod midi;
mod module;
mod plugin;
#[cfg(target_os = "linux")]
mod run_loop;
mod scan;
mod util;

pub use editor::Editor;
pub use midi::MidiEvent;
pub use plugin::{Plugin, Processor};
pub use scan::{PluginInfo, default_search_paths, find_bundles, scan};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot load {0}: {1}")]
    Load(std::path::PathBuf, String),
    #[error("plugin {0}: {1}")]
    Instantiate(String, String),
    #[error("plugin processing: {0}")]
    Process(String),
    #[error("plugin editor: {0}")]
    Editor(String),
}

pub type Result<T> = std::result::Result<T, Error>;
