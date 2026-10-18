//! Non-file sources: apps, settings, emoji, clipboard, browser history. Owner: sources agent.
use ss_core::Doc;
use std::path::PathBuf;

pub fn apps() -> Vec<Doc> {
    Vec::new()
}

/// Omarchy menu entries and system settings, with actions that open them.
pub fn settings() -> Vec<Doc> {
    Vec::new()
}

pub fn emoji() -> Vec<Doc> {
    Vec::new()
}

/// Clipboard history, newest first, with the app and window each clip came from.
pub fn clips(state: &std::path::Path) -> Vec<Doc> {
    let _ = state;
    Vec::new()
}

pub fn history() -> Vec<Doc> {
    Vec::new()
}

/// Files whose change means the sources above must be read again.
pub fn watch_paths() -> Vec<PathBuf> {
    Vec::new()
}

/// Start recording where each new clipboard entry came from (runs for the
/// life of the daemon; writes under `state`).
pub fn track_clip_sources(state: &std::path::Path) {
    let _ = state;
}
