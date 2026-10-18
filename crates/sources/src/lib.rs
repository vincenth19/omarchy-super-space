//! Non-file sources: apps, settings, emoji, clipboard, browser history. Owner: sources agent.
//!
//! Everything here only reads: desktop entries, Omarchy's menu and command
//! list, Omarchy's clipboard history and a copy of Chromium's history. The one
//! file written is the clip-source record under the daemon's state folder.
//! Stamps cover the embedded text only: a Doc whose subtitle or icon changed
//! but whose text did not keeps its stamp (clip and history subtitles carry a
//! relative time such as "2 h ago").
mod apps;
mod clips;
mod emoji;
mod history;
mod settings;

pub use apps::apps;
pub use clips::{clips, track_clip_sources};
pub use emoji::emoji;
pub use history::history;
pub use settings::settings;

use ss_core::{Embed, Locator, Part, doc_text};
use std::path::{Path, PathBuf};

/// Files and folders whose change means the sources above must be read
/// again. Some may not exist yet (no Chromium, no menu extension).
pub fn watch_paths() -> Vec<PathBuf> {
    let mut paths = apps::dirs();
    paths.push(apps::hides_file());
    paths.extend(settings::files());
    paths.push(clips::history_file());
    // The daemon's state folder (DESIGN.md), where track_clip_sources writes.
    paths.push(xdg("XDG_STATE_HOME", ".local/state").join("superspace").join(clips::SOURCES));
    paths.extend(history::files());
    paths
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// An XDG base directory, or its default under $HOME.
fn xdg(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home().join(default))
}

fn omarchy() -> PathBuf {
    std::env::var_os("OMARCHY_PATH").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or("/usr/share/omarchy".into())
}

fn sha1(s: &str) -> String {
    sha1_smol::Sha1::from(s).digest().to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn ago(secs: i64) -> String {
    match secs {
        ..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// One text part embedded as `doc_text(title, body)`, and the stamp of that
/// text. With no body the part is keywords only: on the Pi a short title
/// alone ("X") embedded near short queries such as "volume" (0.718), and the
/// name still matches it.
fn text_part(title: &str, body: String) -> (String, Vec<Part>) {
    let embed = if body.is_empty() { Embed::None } else { Embed::Text(doc_text(title, &body)) };
    let stamp = sha1(if let Embed::Text(t) = &embed { t } else { title });
    (stamp, vec![Part { text: body, locator: Locator::default(), embed }])
}

fn read_json(path: &Path) -> serde_json::Value {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Files under `dir`, at most `depth` levels down, whose name passes `keep`. Sorted.
fn files_under(dir: &Path, depth: usize, keep: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() && depth > 1 => out.extend(files_under(&path, depth - 1, keep)),
            Ok(t) if !t.is_dir() && keep(&entry.file_name().to_string_lossy()) => out.push(path),
            _ => {}
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times() {
        assert_eq!(ago(-5), "just now");
        assert_eq!(ago(61), "1 min ago");
        assert_eq!(ago(2 * 3600 + 5), "2 h ago");
        assert_eq!(ago(3 * 86_400), "3 d ago");
    }

    #[test]
    fn sha1_matches_sha1sum() {
        // printf %s 'https://omarchy.org/plugins' | sha1sum
        assert_eq!(sha1("https://omarchy.org/plugins"), "23fa0cd44b128e1508c384b57a8b18e7264f1b54");
    }
}
