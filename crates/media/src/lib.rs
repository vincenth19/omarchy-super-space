//! Photos, videos and audio: frames, transcripts, lyrics. Owner: media agent.
use ss_core::Doc;
use std::path::Path;

/// Whether `path` is a photo, video or audio file this crate handles.
pub fn is_media(path: &Path) -> bool {
    let _ = path;
    false
}

/// The Doc for a media file. Slow (seconds to minutes): call from the
/// background media queue only. `cache` is where frames and thumbnails go.
pub fn doc(path: &Path, home: &Path, cache: &Path) -> Option<Doc> {
    let _ = (path, home, cache);
    None
}
