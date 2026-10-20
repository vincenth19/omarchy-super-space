//! Photos, videos and audio: frames, transcripts, lyrics. Owner: media agent.
//!
//! The slow work runs in child processes at idle CPU priority: ffmpeg and
//! ffprobe (frames, thumbnails, audio), tesseract (text in screenshots) and
//! whisper.cpp's `whisper-cli` (speech to text). Results are kept under
//! `<cache>/media/<hash of path>/<hash of stamp>/`, so indexing an unchanged
//! file again only reads `parts.json`; a changed file's old results are removed.
//! A tool that runs far longer than the media's length is killed with its children.
//!
//! Speech to text needs, under `$SUPERSPACE_DATA` (default
//! `~/.local/share/superspace`): `whisper/whisper-cli` (the official
//! whisper.cpp Linux build, its libraries beside it), `models/ggml-base-q8_0.bin`
//! and `models/ggml-silero-v6.2.0.bin`. Without them media gets no transcripts.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ss_core::{Action, Doc, Embed, Kind, Locator, Part, doc_text};
use std::fs;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, UNIX_EPOCH};

const IMAGE: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif", "avif", "jxl"];
const VIDEO: &[&str] = &["mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "3gp", "ogv"];
const AUDIO: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wma", "aif", "aiff", "ape", "wv", "mka"];
/// Transcript windows are at most this long (seconds).
const WINDOW: f32 = 15.0;
/// At most this many characters of a long text go to the model.
const EMBED_CHARS: usize = 2000;
/// A tool still running after this many seconds, plus 3 per second of media, is hung and
/// gets killed (whisper on a busy Pi runs near real time). Short in tests, which hang a fake tool.
const GRACE: f32 = if cfg!(test) { 3.0 } else { 120.0 };

fn kind(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    [(IMAGE, Kind::Image), (VIDEO, Kind::Video), (AUDIO, Kind::Audio)]
        .into_iter()
        .find_map(|(list, k)| list.contains(&ext.as_str()).then_some(k))
}

/// Whether `path` is a photo, video or audio file this crate handles.
pub fn is_media(path: &Path) -> bool {
    kind(path).is_some()
}

/// What `doc` puts in `Doc::stamp`, without the slow work: it changes when the
/// file changes, when a song's sibling `.lrc` changes, when a video's frame gap or
/// the way frames are picked changes, and when speech to text gets installed (so media
/// indexed before that gets its transcripts).
pub fn stamp(path: &Path, home: &Path) -> Option<String> {
    let kind = kind(path)?;
    let mtime = |p: &Path| {
        let m = fs::metadata(p).ok().filter(|m| m.is_file())?;
        Some(format!("{}.{}", m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos(), m.len()))
    };
    let mut s = format!("m1-{}", mtime(path)?);
    if kind == Kind::Audio {
        s += "-a2"; // a2: songs told from recordings
        if let Some(l) = mtime(&path.with_extension("lrc")) {
            s += &format!("-lrc{l}");
        }
    }
    if kind == Kind::Video {
        s += &format!("-v3-g{}-{}", frame_gap(0.0), frame_gap(f32::INFINITY)); // v2: near-black frames skipped; v3: gap by length
    }
    if kind != Kind::Image && whisper(home).is_some() {
        s += "-stt";
    }
    Some(s)
}

/// The Doc for a media file. Slow (seconds to minutes): call from the
/// background media queue only. `cache` is where frames and thumbnails go.
pub fn doc(path: &Path, home: &Path, cache: &Path) -> Option<Doc> {
    let kind = kind(path)?;
    let stamp = stamp(path, home)?;
    let dir = results(path, cache, &stamp);
    let saved = match load(&dir) {
        Some(s) => s,
        None => {
            let _ = fs::remove_dir_all(item_dir(path, cache)); // results for an older version of the file
            fs::create_dir_all(&dir).ok()?;
            let (saved, complete) = build(kind, path, home, &dir);
            if complete && let Ok(json) = serde_json::to_vec(&saved) {
                let _ = fs::write(dir.join("parts.json"), json);
            }
            saved
        }
    };
    finish(path, home, kind, stamp, saved)
}

/// `doc` without the slow work and without parts, in microseconds, so new media is found
/// by name at once: the thumbnail, tags and artist too when `doc` already ran on this
/// version of the file. Its stamp ("quick-" + `stamp`) never equals `doc`'s.
pub fn quick_doc(path: &Path, home: &Path, cache: &Path) -> Option<Doc> {
    let kind = kind(path)?;
    let stamp = stamp(path, home)?;
    let saved = load(&results(path, cache, &stamp)).unwrap_or_else(|| Saved { icon: generic(kind), ..Saved::default() });
    finish(path, home, kind, format!("quick-{stamp}"), Saved { parts: Vec::new(), ..saved })
}

/// The audio file a `.lrc` holds the lyrics of (same name, any audio extension), whose
/// `stamp` changes with it. Works after the `.lrc` is deleted, too.
pub fn lrc_owner(lrc: &Path) -> Option<PathBuf> {
    if lrc.extension()? != "lrc" {
        return None;
    }
    let stem = lrc.file_stem()?;
    fs::read_dir(lrc.parent()?).ok()?.flatten().find_map(|e| {
        let p = e.path();
        (e.file_type().is_ok_and(|t| t.is_file()) && p.file_stem() == Some(stem) && kind(&p) == Some(Kind::Audio)).then_some(p)
    })
}

fn finish(path: &Path, home: &Path, kind: Kind, stamp: String, saved: Saved) -> Option<Doc> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let (title, subtitle) = if kind == Kind::Audio {
        audio_text(path, home, &name, &saved)
    } else {
        let folder = match path.parent().map(|p| (p, p.strip_prefix(home))) {
            Some((_, Ok(d))) if d.as_os_str().is_empty() => "~".to_owned(),
            Some((_, Ok(d))) => format!("~/{}", d.display()),
            Some((p, Err(_))) => p.display().to_string(),
            None => String::new(),
        };
        (name.clone(), [Some(folder), saved.artist].into_iter().flatten().collect::<Vec<_>>().join(" · "))
    };
    let mut names = saved.names;
    if title != name {
        names.push(name); // the exact file name still hits
    }
    Some(Doc {
        key: format!("file:{}", path.strip_prefix(home).unwrap_or(path).display()),
        kind,
        title,
        subtitle,
        icon: saved.icon,
        // The whole file: from the start to its end (audio and video), which rows send as `duration`.
        action: Action::Open { path: path.display().to_string(), locator: Locator { t_end: saved.secs.filter(|_| kind != Kind::Image), ..Locator::default() } },
        stamp,
        names,
        parts: saved.parts.into_iter().map(Part::from).collect(),
    })
}

/// A song's title and subtitle ("Keys on the Counter", "Song · Kitchen Radio · 3:42")
/// or a recording's ("2026-10-05 meetup budget.m4a", "Recording · 5 Oct · 0:43").
fn audio_text(path: &Path, home: &Path, name: &str, s: &Saved) -> (String, String) {
    let join = |bits: [Option<String>; 3]| bits.into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let length = s.secs.map(length);
    if !song(path, home, s.music) {
        let day = day(name).or_else(|| s.made.as_deref().and_then(day));
        return (name.to_owned(), join([Some("Recording".into()), day, length]));
    }
    let title = s.title.clone().unwrap_or_else(|| untracked(&path.file_stem().unwrap_or_default().to_string_lossy()).to_owned());
    (title, join([Some("Song".into()), s.artist.clone(), length]))
}

/// A song has music tags or lyrics (`music`), or sits in ~/Music; voice memos, meetings and
/// podcasts (told by their folders) and other audio are recordings: mostly speech.
fn song(path: &Path, home: &Path, music: bool) -> bool {
    let dirs = path.parent().map_or(Path::new(""), |p| p.strip_prefix(home).unwrap_or(p));
    !speech(&dirs.to_string_lossy()) && (music || path.starts_with(home.join("Music")))
}

/// Whether folders or a genre name speech: "Voice Memos", "Recordings", "Podcasts", "Spoken Word".
fn speech(s: &str) -> bool {
    let words = ["memo", "recording", "recorder", "podcast", "meeting", "lecture", "audiobook", "speech", "spoken"];
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| words.contains(&w.trim_end_matches('s')))
}

/// "Keys on the Counter" for "03 - Keys on the Counter", "1-03. …" or "03_…": a track
/// number has two or three digits, so "7 Rings" and "2026-10-05 memo" stay.
fn untracked(stem: &str) -> &str {
    let disc = if stem.as_bytes().get(..2).is_some_and(|b| b[0].is_ascii_digit() && b[1] == b'-') { 2 } else { 0 };
    let n = stem[disc..].len() - stem[disc..].trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let rest = &stem[disc + n..];
    let title = rest.trim_start_matches([' ', '.', '-', '_']);
    if (2..=3).contains(&n) && title.len() < rest.len() && !title.is_empty() { title } else { stem }
}

/// "5 Oct" for text that starts with 2026-10-05, "5 Oct 2025" in another year.
fn day(s: &str) -> Option<String> {
    let b = s.as_bytes().get(..10)?;
    if !b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }) {
        return None;
    }
    let (y, m, d) = (&s[..4], s[5..7].parse::<usize>().ok()?, s[8..10].parse::<u32>().ok()?);
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].get(m.checked_sub(1)?)?;
    let this_year = (1970 + std::time::SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() / 31_556_952).to_string();
    (1..=31).contains(&d).then(|| if y == this_year { format!("{d} {month}") } else { format!("{d} {month} {y}") })
}

/// "0:42", "3:12", "1:02:03".
fn length(secs: f32) -> String {
    let s = secs.round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The 160 px thumbnail of a photo's or video frame's `Embed::Image` (for `Hit::thumb`).
pub fn thumb(image: &Path) -> PathBuf {
    let name = image.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    image.with_file_name(format!("t{}", name.get(1..).unwrap_or_default()))
}

/// Remove a file's frames, thumbnails and saved parts. Call when the file is deleted.
pub fn forget(path: &Path, cache: &Path) {
    let _ = fs::remove_dir_all(item_dir(path, cache));
}

fn item_dir(path: &Path, cache: &Path) -> PathBuf {
    cache.join("media").join(format!("{:016x}", fnv(path.as_os_str().as_encoded_bytes())))
}

/// Where `doc` keeps its results for this version (`stamp`) of the file.
fn results(path: &Path, cache: &Path, stamp: &str) -> PathBuf {
    item_dir(path, cache).join(format!("{:016x}", fnv(stamp.as_bytes())))
}

/// What `doc` saved in `dir`, if it finished there.
fn load(dir: &Path) -> Option<Saved> {
    serde_json::from_slice(&fs::read(dir.join("parts.json")).ok()?).ok()
}

fn generic(kind: Kind) -> String {
    match kind {
        Kind::Image => "image-x-generic",
        Kind::Video => "video-x-generic",
        _ => "audio-x-generic",
    }
    .into()
}

/// FNV-1a: a hash that stays the same across Rust versions, for cache folder names.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Everything slow that `doc` produces, as saved in `parts.json`.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    icon: String,
    artist: Option<String>,
    names: Vec<String>,
    parts: Vec<Piece>,
    /// Audio: the title tag, the length in seconds, the day it was made (creation_time tag),
    /// and whether it has music tags (artist, album) or lyrics and no speech genre.
    title: Option<String>,
    secs: Option<f32>,
    made: Option<String>,
    music: bool,
}

/// A `Part` that can be saved: `image` becomes `Embed::Image`, else `embed` becomes `Embed::Text`.
#[derive(Default, Serialize, Deserialize)]
struct Piece {
    text: String,
    locator: Locator,
    image: Option<PathBuf>,
    embed: Option<String>,
}

impl From<Piece> for Part {
    fn from(p: Piece) -> Part {
        let embed = match (p.image, p.embed) {
            (Some(i), _) => Embed::Image(i),
            (None, Some(t)) => Embed::Text(t),
            (None, None) => Embed::None,
        };
        Part { text: p.text, locator: p.locator, embed }
    }
}

fn at(t: f32, t_end: Option<f32>) -> Locator {
    Locator { t: Some(t), t_end, ..Locator::default() }
}

/// The slow part of `doc`; `complete` is false when a tool failed, so nothing is saved.
fn build(kind: Kind, path: &Path, home: &Path, dir: &Path) -> (Saved, bool) {
    let info = probe(path);
    let duration = info["format"]["duration"].as_str().and_then(|d| d.parse::<f32>().ok());
    // With no duration (some WebM recordings), guess one from the size at 128 kbit/s.
    let secs = duration.unwrap_or_else(|| fs::metadata(path).map_or(0.0, |m| m.len() as f32 / 16_000.0));
    let limit = Duration::from_secs_f32(GRACE + 3.0 * secs);
    let tag = |key: &str| tags(&info).find(|(k, _)| k == key).map(|(_, v)| v);
    let title = tag("title").unwrap_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
    let mut s = Saved { icon: generic(kind), artist: tag("artist"), title: tag("title"), secs: duration, made: tag("creation_time"), ..Saved::default() };
    s.names = ["title", "artist", "album"].into_iter().filter_map(tag).collect();
    s.names.dedup();
    let has = |kind: &str| info["streams"].as_array().is_some_and(|a| a.iter().any(|s| s["codec_type"] == kind));
    let mut complete = true;
    match kind {
        Kind::Image => {
            let image = dir.join("f0001.jpg");
            complete = photo(path, &image, limit);
            if complete {
                s.icon = thumb(&image).display().to_string();
                s.parts.push(Piece { image: Some(image), ..Piece::default() });
            }
            if wants_ocr(path, home)
                && let Some(text) = ocr(path, limit)
            {
                s.parts.push(Piece { embed: Some(doc_text(&title, cut(&text, EMBED_CHARS))), text, ..Piece::default() });
            }
        }
        Kind::Video => {
            let d = duration.unwrap_or(0.0);
            let frames = if has("video") { frames(path, dir, d, frame_gap(d), limit) } else { Some(Vec::new()) };
            complete = frames.is_some();
            let frames = frames.unwrap_or_default();
            if let Some((first, _)) = frames.first() {
                s.icon = thumb(first).display().to_string();
            }
            for (i, (image, t)) in frames.iter().enumerate() {
                let end = frames.get(i + 1).map(|f| f.1).or(duration).filter(|e| e > t);
                s.parts.push(Piece { image: Some(image.clone()), locator: at(*t, end), ..Piece::default() });
            }
            if has("audio") {
                let words = transcript(path, home, dir, &title, limit);
                complete &= words.is_some();
                s.parts.extend(words.unwrap_or_default());
            }
        }
        _ => {
            let lines = lyrics(path, &info);
            s.music = (!lines.is_empty() || s.artist.is_some() || tag("album").is_some()) && !tag("genre").is_some_and(|g| speech(&g));
            if lines.is_empty() {
                let words = transcript(path, home, dir, &title, limit);
                complete = words.is_some();
                s.parts = words.unwrap_or_default();
            } else {
                s.parts = lyric_parts(lines, &title, duration);
            }
        }
    }
    (s, complete)
}

/// `systemd-run` arguments that start a tool in an idle-weight cgroup of its own, with the same
/// PID. Nice values do not cross cgroups, so a niced tool in the daemon's cgroup still starves
/// the model servers. Not expanding `$` keeps file names as they are.
const SCOPE: &[&str] = &["--user", "--scope", "-q", "--collect", "--expand-environment=no", "-p", "CPUWeight=idle", "--"];

/// Whether systemd-run and a user manager can start tools in an idle scope (checked once).
fn scoped() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| Command::new("systemd-run").args(SCOPE).arg("true").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()))
}

/// Run a tool at idle priority, so indexing never slows down the desktop, a search or the
/// index server: in an idle scope when it can, and at nice 19 always. It dies with the
/// daemon even outside the daemon's cgroup. Past `limit` it counts as hung: its process
/// group (it and its children) is killed.
fn run(cmd: &mut Command, limit: Duration) -> Option<Output> {
    unsafe extern "C" {
        fn nice(inc: i32) -> i32;
        fn kill(pid: i32, sig: i32) -> i32;
        #[cfg(target_os = "linux")]
        fn prctl(option: i32, ...) -> i32;
    }
    let tool = cmd.get_program().to_owned();
    let mut scope = Command::new("systemd-run");
    let cmd = if scoped() { scope.args(SCOPE).arg(&tool).args(cmd.get_args()) } else { cmd };
    // SAFETY: nice(2) and prctl(2) are async-signal-safe, so they may run between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            nice(19);
            #[cfg(target_os = "linux")]
            prctl(1, 9); // PR_SET_PDEATHSIG, SIGKILL; kept across systemd-run's exec
            Ok(())
        });
    }
    let child = cmd.process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let group = child.id() as i32; // systemd-run execs the tool, so this is the tool's group
    let (done, wait) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
            eprintln!("ss-media: {} hung for {} s, killed", tool.display(), limit.as_secs());
            // SAFETY: kill(2) touches no memory. The group is the one made above; its leader is
            // reaped only after every pipe closes, so the id cannot have been reused.
            unsafe { kill(-group, 9) };
        }
    });
    let out = child.wait_with_output(); // returns once every process holding the pipes is gone
    drop(done);
    let _ = watchdog.join();
    out.ok().filter(|o| o.status.success())
}

/// ffprobe's view of the file: duration, streams, tags.
fn probe(path: &Path) -> Value {
    run(Command::new("ffprobe").args(["-v", "error", "-of", "json", "-show_format", "-show_streams"]).arg(path), Duration::from_secs_f32(GRACE))
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or(Value::Null)
}

/// Tags of the file and of its streams (Ogg and Opus keep them per stream), keys lowercased.
fn tags(info: &Value) -> impl Iterator<Item = (String, String)> {
    let streams = info["streams"].as_array().into_iter().flatten().map(|s| &s["tags"]);
    std::iter::once(&info["format"]["tags"])
        .chain(streams)
        .filter_map(Value::as_object)
        .flatten()
        .filter_map(|(k, v)| Some((k.to_lowercase(), v.as_str()?.trim().to_owned())))
        .filter(|(_, v)| !v.is_empty())
}

/// The picture scaled to at most 448 px for the model, and its 160 px thumbnail.
fn photo(path: &Path, image: &Path, limit: Duration) -> bool {
    run(Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(path)
        .args(["-filter_complex", &format!("[0:v]{},split[a][b];[b]{}[c]", fit(448), fit(160))])
        .args(["-map", "[a]", "-frames:v", "1", "-q:v", "3"])
        .arg(image)
        .args(["-map", "[c]", "-frames:v", "1", "-q:v", "5"])
        .arg(thumb(image)),
        limit,
    )
    .is_some()
}

/// An ffmpeg scale that fits a frame inside `px`×`px`, never enlarging it.
fn fit(px: u32) -> String {
    format!("scale='min({px},iw)':'min({px},ih)':force_original_aspect_ratio=decrease")
}

/// Screenshots, scans and downloads often hold text worth finding; camera photos rarely do.
fn wants_ocr(path: &Path, home: &Path) -> bool {
    let name = path.to_string_lossy().to_lowercase();
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
        || ["screenshot", "screen shot", "scan"].iter().any(|w| name.contains(w))
        || path.starts_with(home.join("Downloads"))
}

/// Text that tesseract finds, when it looks like real words rather than noise.
fn ocr(path: &Path, limit: Duration) -> Option<String> {
    let out = run(Command::new("tesseract").arg(path).args(["stdout", "--psm", "3"]), limit)?;
    let text = String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" ");
    let words = text.split(' ').filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3).count();
    (words >= 3).then_some(text)
}

/// Seconds between frames of a video `duration` s long: `SUPERSPACE_FRAME_GAP` when set (8 if
/// it is not a number), else 8 for videos over 2 minutes on machines with 4 or fewer cores
/// (the Pi 5 takes ~3.9 s to embed a frame), else 4: 8 s can miss a short clip's best moment.
fn frame_gap(duration: f32) -> f32 {
    match std::env::var("SUPERSPACE_FRAME_GAP") {
        Ok(g) => g.parse().unwrap_or(8.0),
        Err(_) if duration > 120.0 && !std::thread::available_parallelism().is_ok_and(|n| n.get() > 4) => 8.0,
        Err(_) => 4.0,
    }
}

/// Video frames, 448 px, each with its 160 px thumbnail, and their times: the first, one
/// `gap` s after the last and one at each scene change, but at most two per `gap` s on
/// average, so a film full of cuts stays cheap. Past 8 minutes the gap widens: a video
/// gets at most 960 / `gap` frames (120 at 8 s). Near-black frames (mean luma 16 of 255 or
/// less) are never picked, so a cut to a fade from black gets the first frame that shows something.
fn frames(path: &Path, dir: &Path, duration: f32, gap: f32, limit: Duration) -> Option<Vec<(PathBuf, f32)>> {
    let gap = gap * (duration / 480.0).max(1.0);
    // Scaled (to 8-bit full range, where black is 0) before measuring every frame's brightness:
    // on the Pi, 1.4x the old work instead of 2.5x. A fixed format also lets ffmpeg end cleanly with no frames.
    let select = format!(
        "{},format=yuvj420p,signalstats,metadata=select:key=lavfi.signalstats.YAVG:value=16:function=greater,\
         select='isnan(prev_selected_t)+lt(selected_n*{gap},2*t)*(gte(t-prev_selected_t,{gap})+gt(scene,0.3)*gte(t-prev_selected_t,1))'",
        fit(448)
    );
    let out = run(Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-y", "-skip_frame", "noref", "-i"])
        .arg(path)
        .args(["-an", "-sn", "-dn", "-filter_complex", &format!("[0:v]{select},showinfo,split[a][b];[b]{}[c]", fit(160))])
        .args(["-map", "[a]", "-fps_mode", "vfr", "-q:v", "3"])
        .arg(dir.join("f%04d.jpg"))
        .args(["-map", "[c]", "-fps_mode", "vfr", "-q:v", "5"])
        .arg(dir.join("t%04d.jpg")),
        limit,
    )?;
    let log = String::from_utf8_lossy(&out.stderr);
    let times = log
        .lines()
        .filter(|l| l.contains("Parsed_showinfo"))
        .filter_map(|l| l.split_once("pts_time:")?.1.split_whitespace().next()?.parse::<f32>().ok());
    Some(times.enumerate().map(|(i, t)| (dir.join(format!("f{:04}.jpg", i + 1)), t)).collect())
}

/// whisper.cpp's command line tool, model and voice detector, when installed.
fn whisper(home: &Path) -> Option<[PathBuf; 3]> {
    let data = std::env::var_os("SUPERSPACE_DATA").map_or_else(|| home.join(".local/share/superspace"), PathBuf::from);
    let files = [data.join("whisper/whisper-cli"), data.join("models/ggml-base-q8_0.bin"), data.join("models/ggml-silero-v6.2.0.bin")];
    files.iter().all(|f| f.is_file()).then_some(files)
}

/// The spoken words as ~15 s windows: text parts with times, every word in the text.
/// None when a tool failed; empty when there is no speech or no speech to text.
fn transcript(path: &Path, home: &Path, dir: &Path, title: &str, limit: Duration) -> Option<Vec<Piece>> {
    let Some([cli, model, vad]) = whisper(home) else { return Some(Vec::new()) };
    let (wav, out) = (dir.join("audio.wav"), dir.join("audio"));
    run(Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(path).args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"]).arg(&wav), limit)?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(4)).to_string();
    // A low voice threshold keeps sung words; the detector still skips plain music, where
    // whisper would otherwise invent text. Greedy decoding: 2x faster, same words here.
    let done = run(Command::new(&cli)
        .arg("-m")
        .arg(&model)
        .arg("-f")
        .arg(&wav)
        .args(["-t", &threads, "-l", "auto", "-bs", "1", "-bo", "1", "-sns", "-np", "-oj", "--vad", "-vt", "0.1", "-vm"])
        .arg(&vad)
        .arg("-of")
        .arg(&out),
        limit,
    );
    let json = fs::read(out.with_extension("json"));
    let _ = (fs::remove_file(&wav), fs::remove_file(out.with_extension("json")));
    done?;
    // whisper can split a multi-byte character across tokens, so its JSON is not always UTF-8.
    let json: Value = serde_json::from_str(&String::from_utf8_lossy(&json.ok()?)).ok()?;
    let segments = json["transcription"].as_array().into_iter().flatten().filter_map(|s| {
        let secs = |k: &str| s["offsets"][k].as_f64().map(|ms| ms as f32 / 1000.0);
        Some((secs("from")?, secs("to")?, clean(s["text"].as_str()?)))
    });
    let windows = windows(segments).into_iter();
    Some(windows.map(|(t, end, text)| Piece { locator: at(t, Some(end)), embed: Some(doc_text(title, &text)), text, ..Piece::default() }).collect())
}

/// Drop what whisper writes for non-speech: [Music], (laughs), ♪.
fn clean(s: &str) -> String {
    let mut depth = 0i32;
    let kept: String = s
        .chars()
        .filter(|&c| {
            match c {
                '[' | '(' => depth += 1,
                ']' | ')' => depth = (depth - 1).max(0),
                _ => return depth == 0 && c != '♪',
            }
            false
        })
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Join consecutive segments into windows of at most `WINDOW` seconds (a longer segment stays whole).
fn windows(segments: impl IntoIterator<Item = (f32, f32, String)>) -> Vec<(f32, f32, String)> {
    let mut out: Vec<(f32, f32, String)> = Vec::new();
    for (t, end, text) in segments.into_iter().filter(|s| s.2.chars().any(char::is_alphanumeric)) {
        match out.last_mut() {
            Some((start, e, all)) if end - *start <= WINDOW => {
                *e = end;
                all.push(' ');
                all.push_str(&text);
            }
            _ => out.push((t, end, text)),
        }
    }
    out
}

/// A song's lyrics as (time, line): a sibling .lrc first, then an ID3 SYLT frame, then
/// lyrics tags (USLT, LYRICS, ©lyr; often LRC formatted). Untimed lines have no time.
fn lyrics(path: &Path, info: &Value) -> Vec<(Option<f32>, String)> {
    if let Ok(b) = fs::read(path.with_extension("lrc")) {
        let lines = lrc(&String::from_utf8_lossy(&b));
        if !lines.is_empty() {
            return lines;
        }
    }
    let synced = sylt(path);
    if !synced.is_empty() {
        return synced.into_iter().map(|(t, l)| (Some(t), l)).collect();
    }
    let tag = tags(info).find(|(k, _)| k.starts_with("lyrics") || k == "unsyncedlyrics");
    tag.map(|(_, v)| lrc(&v)).unwrap_or_default()
}

/// One part with all the lyrics (what the song is about) and a keyword part per timed line.
fn lyric_parts(lines: Vec<(Option<f32>, String)>, title: &str, duration: Option<f32>) -> Vec<Piece> {
    let all = lines.iter().map(|l| l.1.as_str()).collect::<Vec<_>>().join("\n");
    let mut parts = vec![Piece { embed: Some(doc_text(title, cut(&all, EMBED_CHARS))), text: all, ..Piece::default() }];
    let mut timed: Vec<(f32, String)> = lines.into_iter().filter_map(|(t, l)| Some((t?, l))).collect();
    timed.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (i, (t, line)) in timed.iter().enumerate() {
        let end = timed.get(i + 1).map(|n| n.0).or(duration).filter(|e| e > t);
        parts.push(Piece { text: line.clone(), locator: at(*t, end), ..Piece::default() });
    }
    parts
}

/// LRC lyrics as (time, line); plain lyrics come back untimed. A line with several
/// times ("[00:12.00][01:30.00]chorus") appears once per time; metadata like
/// "[ar:Artist]" and word times ("<00:12.50>") are dropped.
fn lrc(text: &str) -> Vec<(Option<f32>, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let (mut rest, mut times) = (line.trim(), Vec::new());
        while let Some((tag, after)) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            let Some(t) = clock(tag) else {
                if times.is_empty() {
                    rest = ""; // [ar:…], [Chorus]: not a lyric
                }
                break;
            };
            times.push(Some(t));
            rest = after;
        }
        let mut words = String::new();
        for piece in rest.split('<') {
            words += piece.split_once('>').filter(|(t, _)| clock(t).is_some()).map_or(piece, |(_, w)| w);
        }
        let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
        if words.is_empty() {
            continue;
        }
        if times.is_empty() {
            times.push(None);
        }
        out.extend(times.into_iter().map(|t| (t, words.clone())));
    }
    out
}

/// "mm:ss" or "mm:ss.xx" in seconds.
fn clock(s: &str) -> Option<f32> {
    let (m, s) = s.trim().split_once(':')?;
    let (m, s) = (m.parse::<u32>().ok()?, s.parse::<f32>().ok()?);
    (s < 60.0).then_some(m as f32 * 60.0 + s)
}

/// Synchronised lyrics from an ID3v2.3/2.4 SYLT frame with millisecond times.
fn sylt(path: &Path) -> Vec<(f32, String)> {
    let mut head = [0u8; 10];
    let Ok(mut f) = fs::File::open(path) else { return Vec::new() };
    if f.read_exact(&mut head).is_err() || &head[..3] != b"ID3" || !matches!(head[3], 3 | 4) {
        return Vec::new();
    }
    let v4 = head[3] == 4;
    let mut tag = Vec::new();
    let _ = f.take(syncsafe(&head[6..10]) as u64).read_to_end(&mut tag);
    let mut i = match (head[5] & 0x40 != 0, tag.get(..4)) {
        (true, Some(n)) if v4 => syncsafe(n),
        (true, Some(n)) => be32(n) + 4,
        _ => 0,
    };
    while let Some(h) = tag.get(i..i + 10).filter(|h| h[0] != 0) {
        let size = if v4 { syncsafe(&h[4..8]) } else { be32(&h[4..8]) };
        let Some(body) = tag.get(i + 10..i + 10 + size) else { break };
        if &h[..4] == b"SYLT" {
            return sylt_lines(body);
        }
        i += 10 + size;
    }
    Vec::new()
}

/// SYLT body: encoding, language, time format (2 = ms), content type, descriptor, then
/// (text, u32 time) pairs. Karaoke files give syllables and start each line with "\n".
fn sylt_lines(b: &[u8]) -> Vec<(f32, String)> {
    if b.len() < 6 || b[4] != 2 {
        return Vec::new();
    }
    let (_, mut rest) = id3_text(b[0], &b[6..]);
    let mut entries = Vec::new();
    while !rest.is_empty() {
        let (text, r) = id3_text(b[0], rest);
        let Some(ms) = r.get(..4) else { break };
        entries.push((be32(ms) as f32 / 1000.0, text));
        rest = &r[4..];
    }
    let karaoke = entries.iter().any(|(_, s)| s.starts_with(['\n', '\r']));
    let mut lines: Vec<(f32, String)> = Vec::new();
    for (t, s) in entries {
        match lines.last_mut() {
            Some((_, line)) if karaoke && !s.starts_with(['\n', '\r']) => line.push_str(&s),
            _ => lines.push((t, s)),
        }
    }
    lines.into_iter().map(|(t, s)| (t, s.trim().to_owned())).filter(|l| !l.1.is_empty()).collect()
}

/// One terminated ID3 string in encoding `enc` (0 Latin-1, 1 UTF-16 with BOM, 2 UTF-16BE, 3 UTF-8), and the rest.
fn id3_text(enc: u8, b: &[u8]) -> (String, &[u8]) {
    let wide = matches!(enc, 1 | 2);
    let end = if wide { (0..b.len() / 2).map(|k| 2 * k).find(|&k| b[k] == 0 && b[k + 1] == 0) } else { b.iter().position(|&c| c == 0) };
    let (s, rest) = match end {
        Some(e) => (&b[..e], &b[e + if wide { 2 } else { 1 }..]),
        None => (b, &b[b.len()..]),
    };
    let text = match enc {
        0 => s.iter().map(|&c| c as char).collect(),
        3 => String::from_utf8_lossy(s).into_owned(),
        _ => {
            let (le, s) = match s {
                [0xff, 0xfe, r @ ..] => (true, r),
                [0xfe, 0xff, r @ ..] => (false, r),
                _ => (false, s),
            };
            let unit = |c: &[u8; 2]| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) };
            String::from_utf16_lossy(&s.as_chunks::<2>().0.iter().map(unit).collect::<Vec<_>>())
        }
    };
    (text, rest)
}

fn syncsafe(b: &[u8]) -> usize {
    b.iter().fold(0, |n, &x| n << 7 | (x & 0x7f) as usize)
}

fn be32(b: &[u8]) -> usize {
    b.iter().fold(0, |n, &x| n << 8 | x as usize)
}

/// The first `n` characters of `s`.
fn cut(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

#[cfg(test)]
mod tests;
