//! Keeping the index current: a fast queue (apps, settings, clips, text
//! files), a slow queue (media, after all text), the embedder, and a file
//! watcher that settles for 300 ms before indexing what changed.

use crate::embed::{self, Fail, Input, quantize};
use crate::server::Daemon;
use crate::store::{Pending, fnv};
use ss_core::{Doc, Kind};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// The other crates, through their public functions only (fakes under test).
#[cfg(not(test))]
pub mod ext {
    pub use ss_files::{crawl, doc as file_doc, roots, wanted};
    pub use ss_media::{doc as media_doc, is_media};
    pub use ss_sources::{apps, clips, emoji, history, settings, track_clip_sources, watch_paths};
}
#[cfg(test)]
pub use crate::tests::ext;

const SETTLE: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct FastQ {
    pub rescan: bool,
    pub sources: bool,
    pub paths: BTreeSet<PathBuf>,
}

pub fn start(d: &Arc<Daemon>) {
    {
        let mut q = d.fast.lock().unwrap();
        q.rescan = true;
        q.sources = true;
    }
    let run = |f: fn(Arc<Daemon>)| {
        let d = d.clone();
        std::thread::spawn(move || f(d));
    };
    run(fast_loop);
    run(media_loop);
    run(embed_loop);
    run(watch);
    run(|d| ext::track_clip_sources(&d.cfg.state));
    run(|d| loop {
        if let Some(r) = ss_answers::rates(&d.cfg.cache) {
            *d.rates.write().unwrap() = Some(r);
        }
        std::thread::sleep(Duration::from_secs(3600));
    });
    d.fast_cv.notify_all();
}

pub fn file_key(home: &Path, p: &Path) -> String {
    format!("file:{}", p.strip_prefix(home).unwrap_or(p).display())
}

/// "mtime:size": an unchanged file is never read again.
fn file_stamp(p: &Path) -> Option<(String, i64)> {
    let m = std::fs::symlink_metadata(p).ok().filter(|m| m.is_file())?;
    let mtime = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((format!("{}:{}", mtime.as_nanos(), m.len()), mtime.as_secs() as i64))
}

fn excluded(d: &Daemon, p: &Path) -> bool {
    let home = &d.cfg.home;
    d.prefs.read().unwrap().exclude.iter().filter(|r| r.kind.is_none() && r.key.is_none()).filter_map(|r| r.path.as_ref()).any(|x| {
        p.starts_with(x.strip_prefix("~/").map_or_else(|| PathBuf::from(x), |rest| home.join(rest)))
    })
}

/// Under a root ($HOME itself: top level only), not excluded, and wanted by ss-files.
fn wanted(d: &Daemon, p: &Path) -> bool {
    let Some(root) = d.roots.iter().find(|r| p.starts_with(r) && p != r.as_path()) else { return false };
    (*root != d.cfg.home || p.parent() == Some(root)) && !excluded(d, p) && ext::wanted(p)
}

fn fast_loop(d: Arc<Daemon>) {
    loop {
        let job = {
            let mut q = d.fast.lock().unwrap();
            while !q.rescan && !q.sources && q.paths.is_empty() {
                q = d.fast_cv.wait(q).unwrap();
            }
            d.busy.store(true, Ordering::SeqCst);
            std::mem::take(&mut *q)
        };
        let t = Instant::now();
        if job.sources {
            sync_sources(&d);
        }
        if job.rescan {
            rescan(&d);
            eprintln!("superspace: scan done in {:.1}s, {} items", t.elapsed().as_secs_f32(), d.store.mem.read().unwrap().items.len());
        }
        if !job.paths.is_empty() {
            sync_paths(&d, job.paths);
        }
        d.busy.store(false, Ordering::SeqCst);
        d.wake_embedder();
    }
}

/// Apps, settings, emoji, clipboard and history: changed entries are written,
/// missing ones removed.
fn sync_sources(d: &Daemon) {
    type Read = fn(&Daemon) -> Vec<Doc>;
    let sources: [(Kind, Read); 5] = [
        (Kind::App, |_| ext::apps()),
        (Kind::Setting, |_| ext::settings()),
        (Kind::Clip, |d| ext::clips(&d.cfg.state)),
        (Kind::History, |_| ext::history()),
        (Kind::Emoji, |_| ext::emoji()),
    ];
    for (kind, read) in sources {
        let Ok(known) = d.store.stamps(Some(kind)) else { continue };
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        // `[[exclude]] kind = "history"` turns a whole source off.
        let off = d.prefs.read().unwrap().exclude.iter().any(|r| r.kind == Some(kind) && r.path.is_none() && r.key.is_none());
        for doc in if off { Vec::new() } else { read(d) } {
            let stamp = if doc.stamp.is_empty() { format!("h{:x}", fnv(&[&format!("{doc:?}")])) } else { doc.stamp.clone() };
            seen.insert(doc.key.clone());
            if known.get(&doc.key) != Some(&stamp) {
                changed.push((doc, stamp, 0));
            }
        }
        let gone: Vec<String> = known.into_keys().filter(|k| !seen.contains(k)).collect();
        if let Err(e) = d.store.put(changed).and_then(|_| d.store.delete(&gone, &[])) {
            eprintln!("superspace: indexing {kind:?}: {e:#}");
        }
        d.wake_embedder();
    }
}

/// Index one file if it changed: media goes to the slow queue.
fn index_file(d: &Daemon, p: &Path, known: Option<&String>, batch: &mut Vec<(Doc, String, i64)>) {
    let Some((stamp, mtime)) = file_stamp(p) else { return };
    if known == Some(&stamp) {
        return;
    }
    if ext::is_media(p) {
        d.media.lock().unwrap().insert(p.to_path_buf());
        d.media_cv.notify_all();
    } else if let Some(mut doc) = ext::file_doc(p, &d.cfg.home) {
        doc.key = file_key(&d.cfg.home, p);
        batch.push((doc, stamp, mtime));
    }
}

fn flush(d: &Daemon, batch: &mut Vec<(Doc, String, i64)>) {
    if let Err(e) = d.store.put(std::mem::take(batch)) {
        eprintln!("superspace: indexing files: {e:#}");
    }
    d.wake_embedder();
}

fn rescan(d: &Daemon) {
    let Ok(known) = d.store.stamps(None) else { return };
    let mut seen = HashSet::new();
    let mut batch = Vec::new();
    for p in ext::crawl(&d.cfg.home) {
        if excluded(d, &p) {
            continue;
        }
        let key = file_key(&d.cfg.home, &p);
        index_file(d, &p, known.get(&key), &mut batch);
        seen.insert(key);
        if batch.len() >= 32 {
            flush(d, &mut batch);
        }
    }
    flush(d, &mut batch);
    let gone: Vec<String> = known.into_keys().filter(|k| !seen.contains(k)).collect();
    let _ = d.store.delete(&gone, &[]);
}

/// What the watcher saw change. A deleted folder takes everything under it;
/// a new folder (moved or unpacked in) is found by a rescan.
fn sync_paths(d: &Daemon, paths: BTreeSet<PathBuf>) {
    let (mut gone, mut gone_dirs, mut batch, mut new_dir) = (Vec::new(), Vec::new(), Vec::new(), false);
    for p in paths {
        let key = file_key(&d.cfg.home, &p);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() && wanted(d, &p) => new_dir = true,
            Ok(m) if m.is_file() && wanted(d, &p) => {
                let known = d.store.stamp(&key);
                index_file(d, &p, known.as_ref(), &mut batch);
            }
            Ok(m) if m.is_dir() => gone_dirs.push(key + "/"),
            _ => {
                gone_dirs.push(key.clone() + "/");
                gone.push(key);
            }
        }
    }
    flush(d, &mut batch);
    let _ = d.store.delete(&gone, &gone_dirs);
    if new_dir {
        rescan(d);
    }
}

/// Photos, videos and audio, one at a time, once no text waits for the model.
fn media_loop(d: Arc<Daemon>) {
    loop {
        let p = {
            let mut q = d.media.lock().unwrap();
            loop {
                if let Some(p) = q.pop_first() {
                    break p;
                }
                q = d.media_cv.wait(q).unwrap();
            }
        };
        while d.busy.load(Ordering::SeqCst) || d.store.counts().1 > 0 {
            std::thread::sleep(Duration::from_secs(2));
        }
        let key = file_key(&d.cfg.home, &p);
        let Some((stamp, mtime)) = file_stamp(&p) else { continue };
        if d.store.stamp(&key).as_ref() == Some(&stamp) {
            continue;
        }
        if let Some(mut doc) = ext::media_doc(&p, &d.cfg.home, &d.cfg.cache) {
            doc.key = key;
            if let Err(e) = d.store.put(vec![(doc, stamp, mtime)]) {
                eprintln!("superspace: indexing {}: {e:#}", p.display());
            }
            d.wake_embedder();
        }
    }
}

/// Embeds pending parts on the index server: texts in batches, pictures one
/// at a time. A refused batch is retried part by part; refused parts are
/// remembered (not retried until they change); a down server is waited for.
fn embed_loop(d: Arc<Daemon>) {
    let url = d.cfg.index_url.clone();
    let mut down = false;
    let wait = |secs| {
        let g = d.embed_cv.0.lock().unwrap();
        let _ = d.embed_cv.1.wait_timeout(g, Duration::from_secs(secs));
    };
    loop {
        let pending = match d.store.pending(64) {
            Ok(p) if !p.is_empty() => p,
            _ => {
                wait(60);
                continue;
            }
        };
        let mut group = Vec::new();
        let mut chars = 0;
        for p in pending {
            let image = p.image.is_some();
            if !group.is_empty() && (image || group.len() >= 16 || chars + p.text.len() > 12_000) {
                break;
            }
            chars += p.text.len();
            group.push(p);
            if image {
                break;
            }
        }
        let timeout = Duration::from_secs(120);
        let inputs: Vec<Input> = group.iter().map(input).collect();
        let done: Vec<(i64, Option<crate::search::QVec>)> = match embed::embed(&url, &inputs, timeout) {
            Ok(vecs) => group.iter().zip(vecs).map(|(p, v)| (p.id, Some(quantize(&v)))).collect(),
            Err(Fail::Down(e)) => {
                if !std::mem::replace(&mut down, true) {
                    eprintln!("superspace: index server unreachable ({e}); parts wait");
                }
                wait(10);
                continue;
            }
            Err(Fail::Refused(e)) if group.len() == 1 => {
                eprintln!("superspace: model refused part {}: {e}", group[0].id);
                vec![(group[0].id, None)]
            }
            Err(Fail::Refused(_)) => {
                let mut one = Vec::new();
                for p in &group {
                    match embed::embed(&url, &[input(p)], timeout) {
                        Ok(mut v) => one.push((p.id, Some(quantize(&v.remove(0))))),
                        Err(Fail::Refused(e)) => {
                            eprintln!("superspace: model refused part {}: {e}", p.id);
                            one.push((p.id, None));
                        }
                        Err(Fail::Down(_)) => break,
                    }
                }
                one
            }
        };
        down = false;
        if let Err(e) = d.store.set_vecs(&done) {
            eprintln!("superspace: storing vectors: {e:#}");
            wait(10);
        }
    }
}

fn input(p: &Pending) -> Input<'_> {
    match &p.image {
        Some(img) => Input::Image(img),
        None => Input::Text(&p.text),
    }
}

/// inotify (FSEvents on macOS) on the roots, the sources' files and the
/// preferences; changes are handed over once quiet for SETTLE (at most 1 s late).
fn watch(d: Arc<Daemon>) {
    use notify::{RecursiveMode, Watcher};
    let (tx, rx) = channel();
    let mut w = match notify::recommended_watcher(move |e| {
        let _ = tx.send(e);
    }) {
        Ok(w) => w,
        Err(e) => return eprintln!("superspace: no file watcher: {e}"),
    };
    for r in &d.roots {
        let mode = if *r == d.cfg.home { RecursiveMode::NonRecursive } else { RecursiveMode::Recursive };
        if r.is_dir()
            && let Err(e) = w.watch(r, mode)
        {
            eprintln!("superspace: not watching {}: {e}", r.display());
        }
    }
    let prefs_file = d.cfg.prefs();
    let _ = std::fs::create_dir_all(&d.cfg.config);
    let sources = ext::watch_paths();
    let mut dirs = HashSet::new();
    for p in sources.iter().chain([&prefs_file]) {
        let dir = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
        if dir.is_dir() && dirs.insert(dir.clone()) {
            let _ = w.watch(&dir, RecursiveMode::NonRecursive);
        }
    }
    let mut job = FastQ::default();
    let mut prefs = false;
    let mut first: Option<Instant> = None;
    loop {
        let quiet = match rx.recv_timeout(SETTLE) {
            // Opening or reading a file is not a change (inotify reports opens).
            Ok(Ok(ev)) if matches!(ev.kind, notify::EventKind::Access(_)) => first.is_some_and(|t| t.elapsed() > Duration::from_secs(1)),
            Ok(Ok(ev)) => {
                job.rescan |= ev.need_rescan();
                for p in ev.paths {
                    if p == prefs_file {
                        prefs = true;
                    } else if sources.iter().any(|s| p.starts_with(s)) {
                        job.sources = true;
                    } else if d.roots.iter().any(|r| p.starts_with(r)) {
                        job.paths.insert(p);
                    }
                }
                first.get_or_insert_with(Instant::now);
                first.is_some_and(|t| t.elapsed() > Duration::from_secs(1))
            }
            Ok(Err(e)) => {
                eprintln!("superspace: watcher: {e}; rescanning");
                job.rescan = true;
                false
            }
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if !quiet || first.is_none() {
            continue;
        }
        first = None;
        if std::mem::take(&mut prefs) {
            d.reload_prefs();
        }
        let mut q = d.fast.lock().unwrap();
        q.rescan |= job.rescan;
        q.sources |= job.sources;
        q.paths.append(&mut job.paths);
        job = FastQ::default();
        d.fast_cv.notify_all();
    }
}
