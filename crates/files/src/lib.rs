//! Which files to index, and the searchable text inside them. Owner: files agent.
use ss_core::Doc;
use std::path::{Path, PathBuf};

/// Folders to crawl and watch, chosen automatically (see DESIGN.md, "What to index").
pub fn roots(home: &Path) -> Vec<PathBuf> {
    let _ = home;
    Vec::new()
}

/// Whether a file or folder should be indexed at all.
pub fn wanted(path: &Path) -> bool {
    let _ = path;
    false
}

/// Every file under the roots that is wanted.
pub fn crawl(home: &Path) -> Vec<PathBuf> {
    let _ = home;
    Vec::new()
}

/// The Doc for a non-media file: title, icon, action and text parts with locators.
/// Returns None for media files (ss-media handles those) and unreadable files.
pub fn doc(path: &Path, home: &Path) -> Option<Doc> {
    let _ = (path, home);
    None
}
