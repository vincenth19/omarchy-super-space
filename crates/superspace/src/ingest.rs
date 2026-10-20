//! Keeping the index current: a files queue (crawl, watched changes), a
//! sources queue (apps, settings, clips, history, emoji), a media queue (after
//! all text), the embedder, and a file watcher that settles for 300 ms before
//! handing over what changed.

use crate::Cfg;
use crate::embed::{self, Fail, Input, quantize};
use crate::server::Daemon;
use crate::store::{Pending, fnv, prio, record};
use notify::Watcher;
use ss_core::{Doc, Embed, Kind};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// The other crates, through their public functions only (fakes under test).
#[cfg(not(test))]
pub mod ext {
    pub use ss_files::{crawl, crawl_dir, doc as file_doc, icon, roots, wanted_in};
    pub use ss_media::{doc as media_doc, forget, is_media, lrc_owner, quick_doc, stamp as media_stamp};
    pub use ss_sources::{apps, clips_checked, emoji, history_checked, settings, settings_checked, track_clip_sources, watch_paths};

    /// The default browser's desktop id, without ".desktop".
    pub fn default_browser() -> Option<String> {
        let out = std::process::Command::new("xdg-settings").args(["get", "default-web-browser"]).output().ok()?;
        let id = String::from_utf8_lossy(&out.stdout).trim().trim_end_matches(".desktop").to_string();
        (!id.is_empty()).then_some(id)
    }
}
#[cfg(test)]
pub use crate::tests::ext;

const SETTLE: Duration = Duration::from_millis(300);
/// Chromium rewrites History often while browsing: history and clips are read at most this often.
const SLOW_SOURCES: Duration = Duration::from_secs(3);
pub const SOURCES: [Kind; 5] = [Kind::App, Kind::Setting, Kind::Clip, Kind::History, Kind::Emoji];

#[derive(Default)]
pub struct FastQ {
    pub rescan: bool,
    pub paths: BTreeSet<PathBuf>,
    /// Sources to read again.
    pub sources: HashSet<Kind>,
}

pub fn start(d: &Arc<Daemon>) {
    let (tx, rx) = channel();
    let mut sources = ext::watch_paths();
    sources.push(d.cfg.state.join("clip-sources.json"));
    let _ = std::fs::create_dir_all(&d.cfg.config);
    match notify::recommended_watcher(move |e| {
        let _ = tx.send(e);
    }) {
        Ok(mut w) => {
            // The sources' files through their folders (they may not exist yet) and the
            // preferences, before the first scan: on macOS each change of watches restarts the
            // stream, and what changes meanwhile is lost.
            let mut dirs = HashSet::new();
            for p in sources.iter().chain([&d.cfg.prefs()]) {
                let dir = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
                if dir.is_dir() && dirs.insert(dir.clone()) {
                    let _ = w.watch(&dir, notify::RecursiveMode::NonRecursive);
                }
            }
            *d.watcher.lock().unwrap() = Some(w);
        }
        Err(e) => eprintln!("superspace: no file watcher: {e}"),
    }
    {
        let mut q = d.fast.lock().unwrap();
        q.rescan = true;
        q.sources.extend(SOURCES);
    }
    let run = |f: fn(Arc<Daemon>)| {
        let d = d.clone();
        std::thread::spawn(move || f(d));
    };
    run(fast_loop);
    run(sources_loop);
    run(media_loop);
    run(embed_loop);
    let d2 = d.clone();
    std::thread::spawn(move || watch(d2, rx, sources));
    ext::track_clip_sources(&d.cfg.state);
    run(|d| *d.browser.write().unwrap() = ext::default_browser());
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

/// "mtime:size": an unchanged file is never read again (stored with ss-files' version, see `files_version`).
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

/// Indexing threads run at nice 19 with idle I/O (Linux sets both per thread, and the tools a
/// thread starts inherit them), so searches and the desktop come first.
fn background() {
    #[cfg(target_os = "linux")]
    {
        unsafe extern "C" {
            fn gettid() -> i32;
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
            fn syscall(n: i64, ...) -> i64;
        }
        let ioprio_set: i64 = if cfg!(target_arch = "x86_64") { 251 } else { 30 };
        // SAFETY: both only change this thread's own scheduling.
        unsafe {
            let tid = gettid();
            setpriority(0, tid as u32, 19); // PRIO_PROCESS with a thread id: this thread
            syscall(ioprio_set, 1i64, tid as i64, 3i64 << 13); // IOPRIO_WHO_PROCESS, IOPRIO_CLASS_IDLE
        }
    }
}

/// Under a root ($HOME itself: files at the top level only), not excluded, and wanted by ss-files.
fn wanted(d: &Daemon, p: &Path, dir: bool) -> bool {
    let Some(root) = d.roots.iter().find(|r| p.starts_with(r) && p != r.as_path()) else { return false };
    (*root != d.cfg.home || (!dir && p.parent() == Some(root))) && !excluded(d, p) && ext::wanted_in(p, &d.cfg.home)
}

/// Waits until `ready`, then takes the job; `busy` counts it until the caller is done.
fn next_job<T>(d: &Daemon, ready: impl Fn(&FastQ) -> bool, job: impl FnOnce(&mut FastQ) -> T) -> T {
    let mut q = d.fast.lock().unwrap();
    while !ready(&q) {
        q = d.fast_cv.wait(q).unwrap();
    }
    d.busy.fetch_add(1, Ordering::SeqCst);
    job(&mut q)
}

fn fast_loop(d: Arc<Daemon>) {
    background();
    loop {
        let (rescan, paths) = next_job(&d, |q| q.rescan || !q.paths.is_empty(), |q| (std::mem::take(&mut q.rescan), std::mem::take(&mut q.paths)));
        let t = Instant::now();
        if rescan {
            self::rescan(&d);
            eprintln!("superspace: scan done in {:.1}s, {} items", t.elapsed().as_secs_f32(), d.store.mem.read().unwrap().items.len());
        }
        if !paths.is_empty() {
            sync_paths(&d, paths);
        }
        d.busy.fetch_sub(1, Ordering::SeqCst);
        d.wake_embedder();
    }
}

/// Sources are read on their own thread (settings takes ~1.3 s on a Pi), each
/// only when its own files change.
fn sources_loop(d: Arc<Daemon>) {
    background();
    loop {
        let kinds = next_job(&d, |q| !q.sources.is_empty(), |q| std::mem::take(&mut q.sources));
        sync_sources(&d, &kinds);
        d.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Changed entries are written, missing ones removed. A Doc whose text did not
/// change but whose title, subtitle, icon, action or names did is written
/// again too (its vectors are kept).
fn sync_sources(d: &Daemon, kinds: &HashSet<Kind>) {
    for kind in SOURCES.into_iter().filter(|k| kinds.contains(k)) {
        let Ok(known) = d.store.stamps(Some(kind)) else { continue };
        // `[[exclude]] kind = "history"` turns a whole source off.
        let off = d.prefs.read().unwrap().exclude.iter().any(|r| r.kind == Some(kind) && r.path.is_none() && r.key.is_none());
        // None: the source could not be read (a torn copy, a parse error),
        // which must not look like an empty source and delete its rows.
        let docs = match kind {
            _ if off => Some(Vec::new()),
            Kind::App => Some(ext::apps()),
            Kind::Setting => ext::settings_checked(),
            Kind::Clip => ext::clips_checked(&d.cfg.state),
            Kind::History => ext::history_checked(),
            _ => Some(ext::emoji()),
        };
        let Some(docs) = docs else {
            eprintln!("superspace: reading {kind:?} failed; keeping its rows");
            continue;
        };
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for doc in docs {
            let stamp = source_stamp(&doc);
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

/// The source's stamp (of the embedded text) plus everything else shown.
pub fn source_stamp(doc: &Doc) -> String {
    let parts = if doc.stamp.is_empty() { format!("{:?}", doc.parts) } else { String::new() };
    let meta = format!("{:?}{:?}", doc.action, doc.names);
    format!("{}~{:x}", doc.stamp, fnv(&[&doc.title, &doc.subtitle, &doc.icon, &meta, &parts]))
}

/// A Doc stamp's version: what comes before its first "-" (ss-files' "x3-…").
fn version(stamp: &str) -> &str {
    stamp.split('-').next().unwrap_or_default()
}

/// ss-files' extraction version, from a probe file. Each file's stored stamp
/// ends with its Doc's version, so a new extraction reads every file again.
fn files_version(d: &Daemon) -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| {
        let probe = d.cfg.cache.join("probe.txt");
        let _ = std::fs::create_dir_all(&d.cfg.cache).and_then(|_| std::fs::write(&probe, "probe"));
        ext::file_doc(&probe, &d.cfg.home).map_or_else(String::new, |doc| version(&doc.stamp).to_string())
    })
}

/// Index one file if it changed. Media goes to the slow queue; a new one is
/// findable by name at once through its quick Doc (no parts).
fn index_file(d: &Daemon, p: &Path, known: Option<&String>, batch: &mut Vec<(Doc, String, i64)>) {
    let Some((stamp, mtime)) = file_stamp(p) else { return };
    let key = file_key(&d.cfg.home, p);
    if ext::is_media(p) {
        if known.is_some() && known == ext::media_stamp(p, &d.cfg.home).as_ref() {
            return;
        }
        if known.is_none()
            && let Some(doc) = ext::quick_doc(p, &d.cfg.home, &d.cfg.cache)
        {
            let stamp = doc.stamp.clone();
            batch.push((Doc { key, icon: media_icon(p, &doc.icon), ..doc }, stamp, mtime));
        }
        d.media.lock().unwrap().insert(p.to_path_buf());
        d.media_cv.notify_all();
    } else if known != Some(&format!("{stamp}~{}", files_version(d)))
        && let Some(doc) = ext::file_doc(p, &d.cfg.home)
    {
        let stamp = format!("{stamp}~{}", version(&doc.stamp));
        batch.push((Doc { key, ..doc }, stamp, mtime));
    }
}

/// A media row's icon: its thumbnail, else the icon of the app that opens it (DESIGN "Icons").
fn media_icon(p: &Path, icon: &str) -> String {
    if Path::new(icon).is_absolute() { icon.to_string() } else { ext::icon(p) }
}

fn index_all(d: &Daemon, files: &[PathBuf], known: &HashMap<String, String>) {
    let mut batch = Vec::new();
    for p in files.iter().filter(|p| !excluded(d, p)) {
        index_file(d, p, known.get(&file_key(&d.cfg.home, p)), &mut batch);
        if batch.len() >= 32 {
            flush(d, &mut batch);
        }
    }
    flush(d, &mut batch);
}

fn flush(d: &Daemon, batch: &mut Vec<(Doc, String, i64)>) {
    if let Err(e) = d.store.put(std::mem::take(batch)) {
        eprintln!("superspace: indexing files: {e:#}");
    }
    d.wake_embedder();
}

/// Forget items by key or key prefix, with the frames and thumbnails of any media among them,
/// and their place in the media queue.
fn forget(d: &Daemon, keys: &[String], prefixes: &[String]) {
    let exact: HashSet<&String> = keys.iter().collect();
    let gone = |k: &String| exact.contains(k) || prefixes.iter().any(|p| k.starts_with(p.as_str()));
    {
        let mem = d.store.mem.read().unwrap();
        for it in mem.items.values().filter(|it| matches!(it.kind, Kind::Image | Kind::Video | Kind::Audio) && gone(&it.key)) {
            ext::forget(&d.cfg.home.join(&it.key["file:".len()..]), &d.cfg.cache);
        }
    }
    d.media.lock().unwrap().retain(|p| !gone(&file_key(&d.cfg.home, p)));
    if let Err(e) = d.store.delete(keys, prefixes) {
        eprintln!("superspace: forgetting files: {e:#}");
    }
}

/// Files the user uses: picked in Super Space, or in GTK's recently used list.
fn used(d: &Daemon) -> HashSet<String> {
    let xbel = std::fs::read_to_string(d.cfg.home.join(".local/share/recently-used.xbel")).unwrap_or_default();
    let recent = xbel.split("href=\"file://").skip(1).filter_map(|s| s.split('"').next()).map(|u| file_key(&d.cfg.home, Path::new(&unescape(u))));
    d.picks.read().unwrap().iter().map(|p| p.key.clone()).chain(recent).collect()
}

/// A file URI's path, without its %XX escapes and XML's &amp;.
fn unescape(uri: &str) -> String {
    let b = uri.replace("&amp;", "&").into_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).filter(|_| b[i] == b'%').and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        out.push(hex.unwrap_or(b[i]));
        i += if hex.is_some() { 3 } else { 1 };
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn rescan(d: &Daemon) {
    *d.store.used.write().unwrap() = used(d);
    let Ok(known) = d.store.stamps(None) else { return };
    let files: Vec<PathBuf> = ext::crawl(&d.cfg.home).into_iter().filter(|p| !excluded(d, p)).collect();
    index_all(d, &files, &known);
    let seen: HashSet<String> = files.iter().map(|p| file_key(&d.cfg.home, p)).collect();
    let gone: Vec<String> = known.into_keys().filter(|k| !seen.contains(k)).collect();
    forget(d, &gone, &[]);
    // Folders holding wanted files are wanted; only the others are asked.
    let mut dirs = HashSet::new();
    for f in &files {
        for a in f.ancestors().skip(1) {
            if !dirs.insert(a.to_path_buf()) {
                break;
            }
        }
    }
    for r in &d.roots {
        watch_tree(d, r, &dirs);
    }
}

/// Watch `dir` and every wanted folder below it, one watch each, so .git,
/// node_modules and ignored build trees cost no inotify watches. $HOME itself
/// is watched alone (top level only).
fn watch_tree(d: &Daemon, dir: &Path, known: &HashSet<PathBuf>) {
    let mut w = d.watcher.lock().unwrap();
    let Some(w) = w.as_mut() else { return };
    let mut paths = w.paths_mut();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Err(e) = paths.add(&p, notify::RecursiveMode::NonRecursive) {
            eprintln!("superspace: not watching {}: {e}", p.display());
        }
        if p == d.cfg.home {
            continue;
        }
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            let sub = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) && (known.contains(&sub) || wanted(d, &sub, true)) {
                stack.push(sub);
            }
        }
    }
    let _ = paths.commit();
}

/// What the watcher saw change. A deleted folder takes everything under it;
/// a new folder (moved or unpacked in) is watched, then crawled. A song is
/// read again when its .lrc changes.
fn sync_paths(d: &Daemon, paths: BTreeSet<PathBuf>) {
    let (mut gone, mut gone_dirs, mut batch, mut new_dirs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let known = d.store.stamps(None).unwrap_or_default();
    let songs: Vec<PathBuf> = paths.iter().filter_map(|p| ext::lrc_owner(p)).collect();
    for p in paths.into_iter().chain(songs) {
        let key = file_key(&d.cfg.home, &p);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() && wanted(d, &p, true) => new_dirs.push(p),
            Ok(m) if m.is_file() && wanted(d, &p, false) => index_file(d, &p, known.get(&key), &mut batch),
            Ok(m) if m.is_dir() => gone_dirs.push(key + "/"),
            _ => {
                gone_dirs.push(key.clone() + "/");
                gone.push(key);
            }
        }
    }
    flush(d, &mut batch);
    forget(d, &gone, &gone_dirs);
    for dir in new_dirs {
        watch_tree(d, &dir, &HashSet::new());
        index_all(d, &ext::crawl_dir(&dir, &d.cfg.home), &known);
    }
}

/// The media file to read next, with its priority: photos, then songs, then videos, newest first.
pub fn next_media(d: &Daemon) -> Option<(PathBuf, i64)> {
    let q = d.media.lock().unwrap();
    let mem = d.store.mem.read().unwrap();
    let rank = |p: &PathBuf| {
        let it = mem.keys.get(&file_key(&d.cfg.home, p)).map(|id| &mem.items[id]);
        prio(it.map_or(Kind::Video, |it| it.kind), false, false, false, it.map_or(0, |it| it.mtime))
    };
    q.iter().map(|p| (p, rank(p))).min_by_key(|x| x.1).map(|(p, n)| (p.clone(), n))
}

/// Photos, videos and audio, one at a time, once nothing waits for the model.
/// The heavy lock keeps ffmpeg/whisper and picture embedding from running at
/// the same time: on a Pi they slow each other down 2-8x. A file stays queued
/// while it is read (progress counts it), and is read again if it changed meanwhile.
fn media_loop(d: Arc<Daemon>) {
    background();
    loop {
        drop(d.media_cv.wait_while(d.media.lock().unwrap(), |q| q.is_empty()).unwrap());
        while d.busy.load(Ordering::SeqCst) > 0 || d.store.queue().0 > 0 {
            std::thread::sleep(Duration::from_millis(250));
        }
        let Some((p, _)) = next_media(&d) else { continue };
        let key = file_key(&d.cfg.home, &p);
        let stamp = ext::media_stamp(&p, &d.cfg.home);
        if stamp.is_some() && d.store.stamp(&key) != stamp {
            let doc = {
                let _heavy = d.heavy.lock().unwrap();
                ext::media_doc(&p, &d.cfg.home, &d.cfg.cache)
            };
            // Not if the file changed or went while it was read (forget() may be done with it already).
            if let Some(doc) = doc.filter(|_| ext::media_stamp(&p, &d.cfg.home) == stamp) {
                let (stamp, mtime) = (doc.stamp.clone(), file_stamp(&p).map_or(0, |s| s.1));
                if let Err(e) = d.store.put(vec![(Doc { key, icon: media_icon(&p, &doc.icon), ..doc }, stamp, mtime)]) {
                    eprintln!("superspace: indexing {}: {e:#}", p.display());
                }
                d.wake_embedder();
            }
        }
        if ext::media_stamp(&p, &d.cfg.home) == stamp {
            d.media.lock().unwrap().remove(&p);
        }
    }
}

/// Embeds pending parts on the index server: texts in batches, pictures one
/// at a time. A refused batch is retried part by part; refused parts are
/// remembered (not retried until they change); a down server is waited for.
fn embed_loop(d: Arc<Daemon>) {
    background();
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
        // A picture waits while media is being read; text never waits.
        let _heavy = match group[0].image.is_some().then(|| d.heavy.try_lock()) {
            Some(Err(_)) => {
                wait(2);
                continue;
            }
            guard => guard,
        };
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

/// `superspace precompute OUT`: every app, setting and emoji text on this
/// machine and the video labels, embedded through the index server, as a ready-made vector file
/// (compiled in from crates/superspace/data/vectors.bin; see `store::lookup`).
pub fn precompute(cfg: &Cfg, out: &Path) -> anyhow::Result<()> {
    let parts = [ext::apps(), ext::settings(), ext::emoji()].into_iter().flatten().flat_map(|d| d.parts);
    let mut texts: Vec<String> = parts.filter_map(|p| if let Embed::Text(t) = p.embed { Some(t) } else { None }).collect();
    texts.extend(crate::labels::inputs());
    texts.sort();
    texts.dedup();
    write_ready(&cfg.index_url, &texts, out)
}

pub fn write_ready(url: &str, texts: &[String], out: &Path) -> anyhow::Result<()> {
    let mut recs = Vec::new();
    for batch in texts.chunks(16) {
        let inputs: Vec<Input> = batch.iter().map(|t| Input::Text(t)).collect();
        match embed::embed(url, &inputs, Duration::from_secs(300)) {
            Ok(vecs) => recs.extend(batch.iter().zip(vecs).map(|(t, v)| record(t, &v))),
            Err(Fail::Refused(e)) => eprintln!("superspace: skipped {} texts: {e}", batch.len()),
            Err(Fail::Down(e)) => anyhow::bail!("index server {url}: {e}"),
        }
        eprint!("\rsuperspace: {}/{} embedded", recs.len(), texts.len());
    }
    recs.sort_by_key(|r| r.0);
    std::fs::write(out, recs.iter().flat_map(|r| &r.1).copied().collect::<Vec<u8>>())?;
    eprintln!("\nsuperspace: wrote {} vectors to {}", recs.len(), out.display());
    Ok(())
}

fn input(p: &Pending) -> Input<'_> {
    match &p.image {
        Some(img) => Input::Image(img),
        None => Input::Text(&p.text),
    }
}

/// Which source a watched path belongs to (`watch_paths` lists the app
/// folders, the menus and shell.json, the clipboard files and the browsers').
fn source_kind(p: &Path) -> Kind {
    match p.file_name().and_then(|n| n.to_str()) {
        Some("History" | "Bookmarks" | "places.sqlite" | "places.sqlite-wal") => Kind::History,
        Some("clipboard-history.json" | "clip-sources.json") => Kind::Clip,
        Some("omarchy-menu.jsonc" | "shell.json") => Kind::Setting,
        _ => Kind::App,
    }
}

/// Changes worth a sync: under a root (`roots` deepest first), nothing hidden on the way, and in
/// $HOME itself only its own files (not the daemon's database, not Chromium's profile).
pub fn watched(home: &Path, roots: &[PathBuf], p: &Path) -> bool {
    let hidden = p.strip_prefix(home).unwrap_or(p).components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    !hidden && roots.iter().find(|r| p.starts_with(r)).is_some_and(|r| r != home || p.parent() == Some(home))
}

/// Watches the sources' files (through their folders: they may not exist yet)
/// and the preferences; the roots' folders are watched by `rescan`. Changes
/// are handed over once quiet for SETTLE (at most 1 s late); history and
/// clipboard changes at most every SLOW_SOURCES.
fn watch(d: Arc<Daemon>, rx: Receiver<notify::Result<notify::Event>>, sources: Vec<PathBuf>) {
    let prefs_file = d.cfg.prefs();
    let mut job = FastQ::default();
    let (mut prefs, mut first, mut last) = (false, None::<Instant>, Instant::now());
    let (mut slow, mut slow_at) = (HashSet::new(), None::<Instant>);
    loop {
        let timeout = slow_at.map_or(SETTLE, |t| t.saturating_duration_since(Instant::now()).min(SETTLE));
        match rx.recv_timeout(timeout) {
            // Opening or reading a file is not a change (inotify reports opens).
            Ok(Ok(ev)) if matches!(ev.kind, notify::EventKind::Access(_)) => {}
            Ok(Ok(ev)) => {
                let mut hit = ev.need_rescan();
                job.rescan |= hit;
                for p in ev.paths {
                    if p == prefs_file {
                        prefs = true;
                    } else if sources.iter().any(|s| p.starts_with(s)) {
                        match source_kind(&p) {
                            k @ (Kind::History | Kind::Clip) => {
                                slow.insert(k);
                                slow_at.get_or_insert_with(|| Instant::now() + SLOW_SOURCES);
                                continue;
                            }
                            k => _ = job.sources.insert(k),
                        }
                    } else if watched(&d.cfg.home, &d.roots, &p) {
                        job.paths.insert(p);
                    } else {
                        continue;
                    }
                    hit = true;
                }
                if hit {
                    first.get_or_insert_with(Instant::now);
                    last = Instant::now();
                }
            }
            Ok(Err(e)) => {
                eprintln!("superspace: watcher: {e}; rescanning");
                job.rescan = true;
                first.get_or_insert_with(Instant::now);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let settled = first.is_some_and(|t| now - last >= SETTLE || now - t > Duration::from_secs(1));
        let slow_due = slow_at.is_some_and(|t| now >= t);
        if !settled && !slow_due {
            continue;
        }
        if settled {
            first = None;
            if std::mem::take(&mut prefs) {
                d.reload_prefs();
            }
        }
        let mut q = d.fast.lock().unwrap();
        if settled {
            q.rescan |= std::mem::take(&mut job.rescan);
            q.paths.append(&mut job.paths);
            q.sources.extend(job.sources.drain());
        }
        if slow_due {
            q.sources.extend(slow.drain());
            slow_at = None;
        }
        d.fast_cv.notify_all();
    }
}
