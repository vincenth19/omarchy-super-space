//! Tests with a deterministic fake embedder (a tiny HTTP server: bag of words
//! with a few synonyms), fake file adapters and an isolated home per test.

use crate::server::{self, Daemon, RAN};
use crate::{Cfg, client, ingest, search};
use serde_json::{Value, json};
use ss_core::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Stand-ins for the other crates: text files are documents, .jpg and .mp3
/// are media whose slow Doc takes 200 ms (a song's words come from its .lrc);
/// folders are ss-files' own (cheap: no tools).
pub mod ext {
    use ss_core::*;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use crate::ingest::file_key;
    /// Every path `forget` was called with.
    pub static FORGOT: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    pub fn roots(home: &Path) -> Vec<PathBuf> {
        vec![home.join("Documents")]
    }
    pub fn wanted_in(p: &Path, _home: &Path) -> bool {
        p.exists() && !p.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.') || c.as_os_str() == "node_modules")
    }
    pub fn crawl_dir(dir: &Path, home: &Path) -> Vec<PathBuf> {
        let mut out = vec![dir.to_path_buf()];
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if !wanted_in(&p, home) {
            } else if p.is_dir() {
                out.extend(crawl_dir(&p, home))
            } else {
                out.push(p)
            }
        }
        out
    }
    pub fn crawl(home: &Path) -> Vec<PathBuf> {
        crawl_dir(&home.join("Documents"), home)
    }
    pub fn is_media(p: &Path) -> bool {
        p.extension().is_some_and(|e| e == "jpg" || e == "mp3")
    }
    pub fn media_stamp(p: &Path, _home: &Path) -> Option<String> {
        let t = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).map(|t| format!("{t:?}")).ok();
        Some(format!("m{}{}", t(p)?, t(&p.with_extension("lrc")).unwrap_or_default()))
    }
    fn media(p: &Path, home: &Path, stamp: String, parts: Vec<Part>) -> Option<Doc> {
        let name = p.file_name()?.to_string_lossy().into_owned();
        let kind = if p.extension()? == "jpg" { Kind::Image } else { Kind::Audio };
        let action = Action::Open { path: p.display().to_string(), locator: Locator::default() };
        Some(Doc { key: file_key(home, p), kind, title: name, subtitle: String::new(), icon: "image-x-generic".into(), action, stamp, names: Vec::new(), parts })
    }
    pub fn quick_doc(p: &Path, home: &Path, _cache: &Path) -> Option<Doc> {
        media(p, home, format!("quick-{}", media_stamp(p, home)?), Vec::new())
    }
    /// A photo's picture is copied to <cache>/<name>/f0001.jpg, as ss-media names frames. Like
    /// ss-media the stamp is read first, and a file that goes meanwhile still gives a Doc; a
    /// "slow" one takes 1.5 s.
    pub fn media_doc(p: &Path, home: &Path, cache: &Path) -> Option<Doc> {
        let stamp = media_stamp(p, home)?;
        std::thread::sleep(std::time::Duration::from_millis(if p.to_string_lossy().contains("slow") { 1500 } else { 200 }));
        let part = if p.extension()? == "jpg" {
            let img = cache.join(p.file_name()?).join("f0001.jpg");
            std::fs::create_dir_all(img.parent()?).ok()?;
            std::fs::copy(p, &img).ok()?;
            Part { text: String::new(), locator: Locator::default(), embed: Embed::Image(img) }
        } else {
            let text = std::fs::read_to_string(p.with_extension("lrc")).unwrap_or_default();
            Part { embed: Embed::Text(doc_text("song", &text)), text, locator: Locator { t: Some(1.0), ..Locator::default() } }
        };
        media(p, home, stamp, vec![part])
    }
    pub fn forget(p: &Path, _cache: &Path) {
        FORGOT.lock().unwrap().push(p.to_path_buf());
    }
    pub fn icon(p: &Path) -> String {
        format!("app-for-{}", p.extension().unwrap_or_default().to_string_lossy())
    }
    pub fn lrc_owner(lrc: &Path) -> Option<PathBuf> {
        (lrc.extension()? == "lrc").then(|| lrc.with_extension("mp3"))
    }
    pub fn file_doc(p: &Path, home: &Path) -> Option<Doc> {
        if p.is_dir() {
            return ss_files::doc(p, home);
        }
        let text = std::fs::read_to_string(p).ok()?;
        let name = p.file_name()?.to_string_lossy().into_owned();
        Some(Doc {
            key: String::new(),
            kind: Kind::File,
            title: name.clone(),
            subtitle: "~/Documents".into(),
            icon: "text-x-generic".into(),
            action: Action::Open { path: p.display().to_string(), locator: Locator::default() },
            stamp: "f1-x".into(),
            names: Vec::new(),
            parts: vec![Part { embed: Embed::Text(doc_text(&name, &text)), text, locator: Locator { line: Some(1), ..Locator::default() } }],
        })
    }
    pub fn apps() -> Vec<Doc> {
        Vec::new()
    }
    pub fn settings_checked() -> Option<Vec<Doc>> {
        Some(settings())
    }
    pub fn history_checked() -> Option<Vec<Doc>> {
        Some(history())
    }
    pub fn clips_checked(state: &Path) -> Option<Vec<Doc>> {
        Some(clips(state))
    }
    pub fn settings() -> Vec<Doc> {
        Vec::new()
    }
    pub fn emoji() -> Vec<Doc> {
        Vec::new()
    }
    pub fn history() -> Vec<Doc> {
        Vec::new()
    }
    pub fn clips(_: &Path) -> Vec<Doc> {
        Vec::new()
    }
    pub fn watch_paths() -> Vec<PathBuf> {
        Vec::new()
    }
    pub fn track_clip_sources(_: &Path) {}
}

/// Bag of words over 768 hashed dimensions; synonyms share a dimension so
/// "retriever" means "puppy".
fn fake_vec(text: &str) -> Vec<f32> {
    let t = text.replace("task: search result | query: ", " ").replace("title: ", " ").replace(" | text: ", " ");
    let mut v = vec![0f32; 768];
    for w in t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if ["my", "in", "the", "is", "a", "of", "none", "this", "txt", "md", "pdf"].contains(&w) {
            continue;
        }
        let w = match w {
            "puppy" | "dog" | "retriever" | "canine" => "dog",
            "car" | "automobile" | "vehicle" => "car",
            w => w,
        };
        v[(crate::store::fnv(&[w]) % 768) as usize] += 1.0;
    }
    if v.iter().all(|x| *x == 0.0) {
        v[0] = 1.0;
    }
    crate::embed::normalize(v)
}

/// A local HTTP server answering /v1/embeddings like llama-server; queries
/// wait `delay` and are counted in `queries`; any input containing REFUSE
/// fails the whole request; a picture's words are the file's text.
fn fake_embedder(delay: Duration, queries: Arc<AtomicUsize>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/embeddings", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let queries = queries.clone();
            std::thread::spawn(move || serve_fake(s, delay, &queries));
        }
    });
    url
}

fn unbase64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let (mut bits, mut n, mut out) = (0u32, 0, Vec::new());
    for v in s.bytes().filter_map(|c| T.iter().position(|&x| x == c)) {
        bits = (bits << 6 | v as u32) & 0xffff;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn serve_fake(mut s: TcpStream, delay: Duration, queries: &AtomicUsize) {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 65536]);
    let body = loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
            let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).map_or(0, |v| v.trim().parse().unwrap());
            if buf.len() >= end + 4 + len {
                break buf[end + 4..end + 4 + len].to_vec();
            }
        }
    };
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    let picture = |x: &Value| unbase64(x["content"][0]["image_url"]["url"].as_str().and_then(|u| u.split(',').nth(1)).unwrap_or(""));
    let inputs: Vec<String> = v["input"].as_array().into_iter().flatten().map(|x| x.as_str().map_or_else(|| picture(x), String::from)).collect();
    if inputs.iter().any(|t| t.starts_with("task: search result")) {
        queries.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(delay);
    }
    let (status, out) = if inputs.iter().any(|t| t.contains("REFUSE")) {
        (400, r#"{"error":"input is too large"}"#.to_string())
    } else {
        let data: Vec<Value> = inputs.iter().enumerate().map(|(i, t)| json!({"index": i, "embedding": fake_vec(t)})).collect();
        (200, json!({ "data": data }).to_string())
    };
    let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
}

const FAKE_CUTOFFS: &str = "[cutoffs]\napp = 0.3\nsetting = 0.3\nfile = 0.3\naudio = 0.3\nclip = 0.3\nemoji = 0.3\nhistory = 0.3\nframe = 0.5\n";

struct T {
    d: Arc<Daemon>,
    dir: PathBuf,
    /// Query embeddings asked for so far.
    queries: Arc<AtomicUsize>,
}

fn start(delay_ms: u64, prefs: &str) -> T {
    start_with(delay_ms, prefs, None, |_| {})
}

/// `index_url` replaces the index server (e.g. one that is down); `setup` runs on the home before the daemon starts.
fn start_with(delay_ms: u64, prefs: &str, index_url: Option<&str>, setup: impl FnOnce(&std::path::Path)) -> T {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("ss{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home/Documents")).unwrap();
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("config/preferences.toml"), format!("{prefs}\n{FAKE_CUTOFFS}")).unwrap();
    setup(&dir.join("home"));
    let queries = Arc::new(AtomicUsize::new(0));
    let url = fake_embedder(Duration::from_millis(delay_ms), queries.clone());
    let cfg = Cfg {
        home: dir.join("home"),
        state: dir.join("state"),
        cache: dir.join("cache"),
        config: dir.join("config"),
        sock: dir.join("s.sock"),
        query_url: url.clone(),
        index_url: index_url.map_or(url, String::from),
    };
    let d = Daemon::open(cfg).unwrap();
    let l = server::bind(&d.cfg.sock).unwrap();
    ingest::start(&d);
    let d2 = d.clone();
    std::thread::spawn(move || server::listen(d2, l));
    let t = T { d, dir, queries };
    until("first scan", Duration::from_secs(5), || t.idle());
    t
}

fn doc(key: &str, kind: Kind, title: &str, body: &str, names: &[&str]) -> Doc {
    let action = match kind {
        Kind::App => Action::Launch { desktop_id: key.trim_start_matches("app:").into() },
        _ => Action::Open { path: format!("/home/u/{}", key.trim_start_matches("file:")), locator: Locator::default() },
    };
    Doc {
        key: key.into(),
        kind,
        title: title.into(),
        subtitle: String::new(),
        icon: "x".into(),
        action,
        stamp: "1".into(),
        names: names.iter().map(|s| s.to_string()).collect(),
        parts: vec![Part { text: body.into(), locator: Locator { line: Some(1), ..Locator::default() }, embed: Embed::Text(doc_text(title, body)) }],
    }
}

fn until(what: &str, limit: Duration, mut ok: impl FnMut() -> bool) -> Duration {
    let t = Instant::now();
    while !ok() {
        assert!(t.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
    t.elapsed()
}

impl T {
    fn idle(&self) -> bool {
        let q = self.d.fast.lock().unwrap();
        !q.rescan && q.sources.is_empty() && q.paths.is_empty() && self.d.busy.load(Ordering::SeqCst) == 0
    }
    fn put(&self, docs: Vec<Doc>) {
        self.d.store.put(docs.into_iter().map(|d| (d, "1".into(), 0)).collect()).unwrap();
        self.d.wake_embedder();
        until("embedding", Duration::from_secs(5), || self.d.store.counts().0 == 0);
    }
    fn search(&self, q: &str) -> (Vec<Row>, bool) {
        search::search(&self.d, q, 30, self.d.query_vec(q).as_ref(), None, false)
    }
    fn keys(&self, q: &str) -> Vec<String> {
        self.search(q).0.into_iter().map(|r| r.key).collect()
    }
    fn req(&self, r: Value) -> Value {
        match self.d.handle(serde_json::from_value(r).unwrap()) {
            Response::Value { value, .. } => value,
            other => panic!("{}", serde_json::to_string(&other).unwrap()),
        }
    }
}

fn corpus(t: &T) {
    t.put(vec![
        doc("app:terminal", Kind::App, "Terminal", "Use the command line", &["console", "shell"]),
        doc("app:firefox", Kind::App, "Firefox", "Browse the web", &["web browser"]),
        doc("file:Documents/lease.pdf", Kind::File, "lease.pdf", "The rent is due monthly. This lease ends in June.", &[]),
        doc("file:Documents/puppy.txt", Kind::File, "puppy.txt", "my puppy plays fetch in the park", &[]),
        doc("file:Documents/notes.md", Kind::File, "notes.md", "shopping list eggs milk", &[]),
    ]);
}

#[test]
fn fts5_is_compiled_in() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    let on: i64 = c.query_row("select sqlite_compileoption_used('ENABLE_FTS5')", [], |r| r.get(0)).unwrap();
    assert_eq!(on, 1);
}

#[test]
fn names_keywords_and_meaning_rank_as_designed() {
    let t = start(0, "");
    corpus(&t);
    let (rows, _) = t.search("term");
    assert_eq!((rows[0].key.as_str(), rows[0].why), ("app:terminal", Why::Name));
    assert_eq!(t.keys("console")[0], "app:terminal", "names match too");
    assert_eq!(t.keys("lease.pdf")[0], "file:Documents/lease.pdf", "exact file names hit");
    let (rows, empty) = search::search(&t.d, "rent due", 30, None, None, false);
    assert!(!empty);
    assert_eq!((rows[0].key.as_str(), rows[0].why), ("file:Documents/lease.pdf", Why::Keyword));
    assert_eq!(t.keys("rent due")[0], "file:Documents/lease.pdf");
    let sn = rows[0].snippet.as_ref().unwrap();
    let marked: Vec<String> = sn.marks.iter().map(|&(a, b)| sn.text.chars().skip(a).take(b - a).collect()).collect();
    assert_eq!(marked, ["rent", "due"]);
    // Meaning only: no shared word, so the instant phase has nothing local.
    let (rows, empty) = search::search(&t.d, "retriever", 30, None, None, false);
    assert!(empty && rows.len() == 1);
    let (rows, empty) = t.search("retriever");
    assert!(!empty);
    assert_eq!((rows[0].key.as_str(), rows[0].why), ("file:Documents/puppy.txt", Why::Meaning));
    assert_eq!(rows.len(), 2, "unrelated items are cut, not padded: {:?}", rows.iter().map(|r| &r.key).collect::<Vec<_>>());
}

#[test]
fn web_row_is_always_last_and_alone_when_nothing_matches() {
    let t = start(0, "");
    corpus(&t);
    for q in ["term", "rent", "retriever", "e", "zxqv wobble"] {
        let (rows, empty) = t.search(q);
        assert_eq!(rows.last().unwrap().kind, Kind::Web, "{q}");
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Web).count(), 1);
        assert_eq!(empty, rows.len() == 1, "{q}");
    }
    let (rows, empty) = t.search("zxqv wobble");
    assert!(empty);
    assert_eq!(rows[0].title, "Search the web for “zxqv wobble”");
    assert_eq!(rows[0].action, Action::Url { url: "https://duckduckgo.com/?q=zxqv%20wobble".into() });
}

#[test]
fn pins_boosts_burials_and_exclusions() {
    let t = start(0, "");
    t.put(vec![
        doc("file:Documents/a.txt", Kind::File, "a.txt", "quarterly report draft", &[]),
        doc("file:Documents/b.txt", Kind::File, "b.txt", "annual report final", &[]),
    ]);
    let a = "file:Documents/a.txt";
    let b = "file:Documents/b.txt";
    t.req(json!({"op": "pref", "id": 1, "change": {"pin": {"q": "Report", "key": b}}}));
    let (rows, _) = t.search("report");
    assert_eq!((rows[0].key.as_str(), rows[0].why), (b, Why::Pinned));
    t.req(json!({"op": "pref", "id": 1, "change": {"unpin": {"q": "Report"}}}));
    t.req(json!({"op": "pref", "id": 1, "change": {"bury": {"key": b, "by": 0.5}}}));
    assert_eq!(t.keys("report")[..2], [a, b]);
    t.req(json!({"op": "pref", "id": 1, "change": {"unbury": {"key": b}}}));
    t.req(json!({"op": "pref", "id": 1, "change": {"boost": {"path": "~/Documents/b.txt", "by": 0.5}}}));
    assert_eq!(t.keys("report")[..2], [b, a]);
    t.req(json!({"op": "pref", "id": 1, "change": {"exclude": {"key": a}}}));
    assert!(!t.keys("report").contains(&a.to_string()));
    let p = t.req(json!({"op": "prefs", "id": 1}));
    assert_eq!(p["exclude"][0]["key"], a);
    assert!(std::fs::read_to_string(t.d.cfg.prefs()).unwrap().contains("[[boost]]"));
}

#[test]
fn preferences_file_is_hot_reloaded() {
    let t = start(0, "");
    t.put(vec![
        doc("file:Documents/a.txt", Kind::File, "a.txt", "quarterly report draft", &[]),
        doc("file:Documents/b.txt", Kind::File, "b.txt", "annual report final", &[]),
    ]);
    std::thread::sleep(Duration::from_millis(500)); // the watcher is up
    let pin = "[[pin]]\nq = \"report\"\nkey = \"file:Documents/a.txt\"\n";
    std::fs::write(t.d.cfg.prefs(), format!("{pin}{FAKE_CUTOFFS}")).unwrap();
    let took = until("pin from file", Duration::from_secs(3), || t.search("report").0[0].why == Why::Pinned);
    eprintln!("preferences reloaded in {took:?}");
}

#[test]
fn picks_are_learned_per_prefix() {
    let t = start(0, "");
    t.put(vec![
        doc("app:terminal", Kind::App, "Terminal", "command line", &[]),
        doc("app:telegram", Kind::App, "Telegram", "messages", &[]),
    ]);
    let other = |first: &str| if first == "app:terminal" { "app:telegram" } else { "app:terminal" };
    let picked = other(&t.keys("te")[0]).to_string();
    let v = t.req(json!({"op": "activate", "id": 1, "key": picked, "q": "te"}));
    let id = picked.trim_start_matches("app:");
    assert_eq!(v["argv"], json!(["gtk-launch", id]));
    assert!(RAN.lock().unwrap().iter().any(|a| a == &["gtk-launch", id]));
    until("the pick wins its prefix", Duration::from_secs(2), || t.keys("te")[0] == picked);
    assert_eq!(t.keys("t")[0], picked, "and shorter prefixes");
    let (rows, _) = t.search("");
    assert_eq!((rows[0].key.as_str(), rows[0].why), (picked.as_str(), Why::Recent), "empty query lists recent picks");
    assert!(t.d.cfg.state.join("weights.json").exists());
}

fn read_responses(r: &mut impl BufRead, until: Instant) -> Vec<Value> {
    let mut out = Vec::new();
    let mut line = String::new();
    while Instant::now() < until {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => out.push(serde_json::from_str(&line).unwrap()),
            Err(_) => {} // read timeout: keep listening until `until`
        }
    }
    out
}

#[test]
fn socket_sends_two_phases_and_skips_superseded_finals() {
    let t = start(300, "");
    corpus(&t);
    let mut s = std::os::unix::net::UnixStream::connect(&t.d.cfg.sock).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(400))).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    writeln!(s, r#"{{"op":"search","id":1,"q":"retriever"}}"#).unwrap();
    let got = read_responses(&mut r, Instant::now() + Duration::from_secs(2));
    let phases: Vec<_> = got.iter().map(|v| (v["id"].as_u64().unwrap(), v["phase"].as_str().unwrap().to_string())).collect();
    assert_eq!(phases, [(1, "instant".to_string()), (1, "final".to_string())]);
    assert_eq!(got[0]["empty"], true);
    assert_eq!(got[1]["rows"][0]["key"], "file:Documents/puppy.txt");
    assert!(got[1].get("progress").is_none(), "no progress once everything is embedded");
    assert!(got[1]["timing"]["embed_ms"].as_f64().unwrap() >= 250.0);
    for (id, q) in [(2, "alpha"), (3, "alphab"), (4, "alphabet")] {
        writeln!(s, r#"{{"op":"search","id":{id},"q":"{q}"}}"#).unwrap();
    }
    let got = read_responses(&mut r, Instant::now() + Duration::from_secs(3));
    let finals: Vec<u64> = got.iter().filter(|v| v["phase"] == "final").map(|v| v["id"].as_u64().unwrap()).collect();
    let instants: Vec<u64> = got.iter().filter(|v| v["phase"] == "instant").map(|v| v["id"].as_u64().unwrap()).collect();
    assert_eq!(instants, [2, 3, 4]);
    assert_eq!(finals, [4], "only the newest search gets its final phase");
    // Other requests share the connection.
    writeln!(s, r#"{{"op":"status","id":9}}"#).unwrap();
    writeln!(s, "not json").unwrap();
    let got = read_responses(&mut r, Instant::now() + Duration::from_secs(1));
    assert_eq!(got[0]["value"]["items"], 6, "the corpus and the Documents folder");
    assert!(got[1]["error"].as_str().unwrap().starts_with("bad request"));
}

#[test]
fn watcher_indexes_and_forgets_files_within_2s() {
    let t = start(0, "");
    std::thread::sleep(Duration::from_millis(500)); // initial scan and watches
    let f = t.dir.join("home/Documents/new/zebra.txt");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "zebra crossing near the school").unwrap();
    let key = "file:Documents/new/zebra.txt";
    let has = |t: &T| search::search(&t.d, "zebra crossing", 30, None, None, false).0.iter().any(|r| r.key == key);
    let added = until("new file", Duration::from_secs(2), || has(&t));
    assert!(t.d.store.stamp(key).unwrap().ends_with("~f1"), "the stamp holds the Doc's version: a new extraction reads it again");
    std::fs::write(&f, "now about giraffes").unwrap();
    let changed = until("changed file", Duration::from_secs(2), || !has(&t));
    std::fs::remove_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(t.dir.join("home/Documents/keep.txt"), "zebra crossing again").unwrap();
    until("deleted folder", Duration::from_secs(2), || {
        let k = t.keys("zebra crossing");
        k.contains(&"file:Documents/keep.txt".to_string()) && !k.contains(&key.to_string())
    });
    eprintln!("searchable after {added:?}, change seen after {changed:?}");
}

#[test]
fn refused_parts_are_remembered_not_retried() {
    let t = start(0, "");
    let bad = doc("file:Documents/huge.txt", Kind::File, "huge.txt", "REFUSE this one", &[]);
    t.put(vec![doc("file:Documents/ok.txt", Kind::File, "ok.txt", "fine words", &[]), bad.clone()]);
    let (pending, _, refused) = t.d.store.counts();
    assert_eq!((pending, refused), (0, 1));
    assert_eq!(t.d.store.mem.read().unwrap().rows.len(), 1);
    // Re-indexed with the same text: still refused, never sent again.
    t.d.store.put(vec![(bad, "2".into(), 0)]).unwrap();
    assert_eq!(t.d.store.counts(), (0, 0, 1));
    assert!(t.keys("huge").contains(&"file:Documents/huge.txt".to_string()), "still found by name");
}

#[test]
fn explain_read_and_similar() {
    let t = start(0, "");
    corpus(&t);
    let e = t.req(json!({"op": "explain", "id": 1, "q": "rent", "key": "file:Documents/lease.pdf"}));
    assert_eq!(e["rank"], 0);
    assert!(e["features"]["keyword"]["contribution"].as_f64().unwrap() > 0.0);
    let e = t.req(json!({"op": "explain", "id": 1, "q": "rent", "key": "app:firefox"}));
    assert_eq!((e["kept"].as_bool(), e["rank"].is_null()), (Some(false), true));
    let r = t.req(json!({"op": "read", "id": 1, "key": "file:Documents/lease.pdf"}));
    assert_eq!(r["parts"][0]["text"], "The rent is due monthly. This lease ends in June.");
    let s = t.req(json!({"op": "similar", "id": 1, "key": "file:Documents/puppy.txt"}));
    assert!(s["rows"].as_array().unwrap().iter().all(|r| r["key"] != "file:Documents/puppy.txt"));
}

#[test]
fn mcp_matches_the_cli() {
    let t = start(0, "");
    corpus(&t);
    let input = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "search", "arguments": {"q": "rent", "kinds": ["file"]}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "pref", "arguments": {"change": {"prefer": "dogs"}}}}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let mut out = Vec::new();
    client::mcp_loop(&t.d.cfg, input.as_bytes(), &mut out).unwrap();
    let resp: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(resp.iter().map(|r| r["id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2, 3, 4]);
    assert_eq!(resp[1]["result"]["tools"].as_array().unwrap().len(), 6);
    let mcp: Value = serde_json::from_str(resp[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let cli = client::search(&t.d.cfg, "rent", Some("file"), 10).unwrap();
    let keys = |rows: &Value| rows.as_array().unwrap().iter().map(|r| r["key"].clone()).collect::<Vec<_>>();
    assert_eq!(keys(&mcp["rows"]), keys(&cli["rows"]));
    assert_eq!(keys(&mcp["rows"]), [json!("file:Documents/lease.pdf")]);
    assert!(resp[3]["result"]["content"][0]["text"].as_str().unwrap().contains("dogs"));
    assert_eq!(t.d.prefer.read().unwrap().len(), 1, "prefer sentence embedded");
}

#[test]
fn snippet_marks_query_words_at_word_starts() {
    let long = format!("{} the golden retriever ran", "word ".repeat(40));
    let s = search::snippet(&long, &["golden".into(), "retr".into()]);
    let marked: Vec<String> = s.marks.iter().map(|&(a, b)| s.text.chars().skip(a).take(b - a).collect()).collect();
    assert_eq!(marked, ["golden", "retr"]);
    assert!(s.text.starts_with("…word "), "cut between words: {}", s.text);
    let s = search::snippet(&"Residential tenancy 31 ".repeat(20), &[]);
    assert!(s.text.ends_with('…') && s.text.chars().count() <= 161, "{}", s.text);
    assert!(s.text.trim_end_matches('…').split(' ').all(|w| ["Residential", "tenancy", "31"].contains(&w)), "never mid-word: {}", s.text);
    assert!(search::snippet("übergolden", &["golden".into()]).marks.is_empty());
    let s = search::snippet(&format!("golden {long}"), &["golden".into(), "retr".into()]);
    assert!(s.text.ends_with("the golden retriever ran") && s.marks.len() == 2, "where both words are, not the first word: {}", s.text);
}

#[test]
fn actions_map_to_commands() {
    let open = |path: &str, locator: Locator| Action::Open { path: path.into(), locator };
    let t = |s: f32| Locator { t: Some(s), ..Locator::default() };
    assert_eq!(server::command(&open("/v/a.mp4", t(12.0)), Kind::Video), ["mpv", "--start=12", "/v/a.mp4"]);
    assert_eq!(server::command(&open("/d/x.pdf", Locator::default()), Kind::File), ["xdg-open", "/d/x.pdf"]);
    assert_eq!(server::command(&Action::Launch { desktop_id: "firefox".into() }, Kind::App), ["gtk-launch", "firefox"]);
    assert_eq!(server::command(&Action::Copy { text: "hi".into() }, Kind::Clip), ["wl-copy", "--", "hi"]);
}

/// cargo test --release -- --ignored bench: the 50k-part instant budget.
#[test]
#[ignore]
fn bench_50k_parts_instant_under_15ms() {
    let dir = std::env::temp_dir().join("ss-bench");
    let cfg = Cfg { state: dir.clone(), ..Cfg::from_env() };
    let worst = crate::bench::run(&cfg, 50_000).unwrap();
    assert!(worst < 15.0, "instant p95 {worst} ms");
}

#[test]
fn calm_typing_embeds_once_per_pause() {
    let t = start(0, "");
    corpus(&t);
    let mut s = std::os::unix::net::UnixStream::connect(&t.d.cfg.sock).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let sent = Instant::now();
    for (id, q) in [(1, "pup"), (2, "pupp"), (3, "puppy")] {
        writeln!(s, r#"{{"op":"search","id":{id},"q":"{q}"}}"#).unwrap();
        std::thread::sleep(Duration::from_millis(60));
    }
    let got = read_responses(&mut r, Instant::now() + Duration::from_millis(600));
    let finals: Vec<&Value> = got.iter().filter(|v| v["phase"] == "final").collect();
    assert_eq!(finals.iter().map(|v| v["id"].as_u64().unwrap()).collect::<Vec<_>>(), [3]);
    let ms = |k: &str| finals[0]["timing"][k].as_f64().unwrap();
    assert!(ms("wait_ms") >= 140.0 && ms("total_ms") - ms("wait_ms") < 100.0, "the final waits for a 150 ms pause, and says so");
    assert_eq!(t.queries.load(Ordering::SeqCst), 1, "superseded searches are never embedded");
    assert!(sent.elapsed() < Duration::from_secs(2));
    // A client that has said all it will say gets its final at once.
    let t0 = Instant::now();
    assert_eq!(client::search(&t.d.cfg, "puppy plays", None, 5).unwrap()["phase"], "final");
    assert!(t0.elapsed() < Duration::from_millis(120), "the CLI waited {:?}", t0.elapsed());
}

#[test]
fn keyword_and_names_work_before_embeddings() {
    let t = start_with(0, "", Some("http://127.0.0.1:9/v1/embeddings"), |_| {});
    std::thread::sleep(Duration::from_millis(500));
    std::fs::write(t.dir.join("home/Documents/bond.txt"), "a rental bond of two thousand four hundred dollars").unwrap();
    until("keyword match with no vectors", Duration::from_secs(2), || {
        t.keys("two thousand four hundred dollars").contains(&"file:Documents/bond.txt".to_string())
    });
    assert_eq!(t.keys("bond.txt")[0], "file:Documents/bond.txt", "and by name");
    assert!(t.d.store.counts().0 > 0, "its vectors are still pending");
    assert!(t.search("security deposit").1, "meaning waits for the vectors");
}

#[test]
fn media_is_found_by_name_at_once_then_by_content_and_forgotten() {
    let t = start(0, "");
    std::thread::sleep(Duration::from_millis(500));
    let docs = t.dir.join("home/Documents");
    std::fs::write(docs.join("beach-day.jpg"), "golden retriever swimming").unwrap();
    std::fs::write(docs.join("rain.lrc"), "clouds over the harbour").unwrap();
    std::fs::write(docs.join("rain.mp3"), "audio").unwrap();
    let (photo, song) = ("file:Documents/beach-day.jpg", "file:Documents/rain.mp3");
    until("names before the slow pass", Duration::from_millis(1500), || {
        t.keys("beach-day").first().map(String::as_str) == Some(photo) && t.keys("rain.mp3").first().map(String::as_str) == Some(song)
    });
    until("photo by meaning", Duration::from_secs(5), || t.keys("retriever").first().map(String::as_str) == Some(photo));
    let (rows, _) = t.search("retriever");
    let thumb = rows[0].hits[0].thumb.as_deref().unwrap();
    assert!(thumb.ends_with("beach-day.jpg/t0001.jpg"), "hit thumb is the small picture: {thumb}");
    until("song words", Duration::from_secs(5), || t.keys("harbour").contains(&song.to_string()));
    assert_eq!(t.search("rain.mp3").0[0].icon, "app-for-mp3", "audio wears the icon of the app that plays it");
    std::fs::write(docs.join("rain.lrc"), "sunshine on the river").unwrap();
    until("song read again after its .lrc changed", Duration::from_secs(5), || t.keys("sunshine").contains(&song.to_string()));
    std::fs::remove_file(docs.join("beach-day.jpg")).unwrap();
    until("deleted photo", Duration::from_secs(2), || !t.keys("beach-day").contains(&photo.to_string()));
    assert!(ext::FORGOT.lock().unwrap().contains(&docs.join("beach-day.jpg")), "its frames are forgotten");
}

#[test]
fn every_wanted_folder_is_watched_but_not_ignored_ones() {
    let t = start_with(0, "", None, |home| std::fs::create_dir_all(home.join("Documents/empty/deeper")).unwrap());
    std::thread::sleep(Duration::from_millis(500));
    let docs = t.dir.join("home/Documents");
    std::fs::write(docs.join("empty/deeper/otter.txt"), "otter facts").unwrap();
    until("file in a folder that was empty at start", Duration::from_secs(2), || t.keys("otter").len() == 2);
    std::fs::create_dir_all(docs.join("proj/node_modules/pkg")).unwrap();
    std::fs::create_dir_all(docs.join("proj/src/deep")).unwrap();
    std::fs::write(docs.join("proj/node_modules/pkg/badger.txt"), "badger").unwrap();
    std::fs::write(docs.join("proj/src/deep/badger.txt"), "badger").unwrap();
    until("new folder crawled", Duration::from_secs(2), || t.keys("badger").len() == 2);
    std::fs::write(docs.join("proj/src/deep/later.txt"), "badger later").unwrap();
    until("new subfolder watched", Duration::from_secs(2), || t.keys("badger").len() == 3);
    assert!(!t.keys("badger").iter().any(|k| k.contains("node_modules")));
}

#[test]
fn folders_are_found_by_name_opened_and_follow_the_watcher() {
    let t = start_with(0, "", None, |home| {
        for d in ["Documents/Meetup/node_modules/pkg", "Documents/.secret/Meetup", "Documents/Videos"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::fs::write(home.join("Documents/Meetup/notes.txt"), "we agreed to hold the meetup at the library").unwrap();
    });
    let meetup = "file:Documents/Meetup";
    let (rows, _) = t.search("meetup");
    let r = &rows[0];
    assert_eq!((r.key.as_str(), r.kind, r.title.as_str(), r.subtitle.as_str(), r.icon.as_str()), (meetup, Kind::Folder, "Meetup", "~/Documents", "folder"));
    assert_eq!(t.keys("documents")[0], "file:Documents", "a root is a folder too");
    assert!(t.d.store.stamp("file:Documents/Meetup/node_modules").is_none() && !t.keys("meetup").iter().any(|k| k.contains(".secret")), "excluded folders stay out");
    // Names only: a folder named in a question ("meetup") or by a type word ("Videos") never crowds out meaning.
    until("embedded", Duration::from_secs(5), || t.d.store.counts().0 == 0);
    let q = "where did we agree to hold the meetup";
    assert_eq!(t.keys(q), ["file:Documents/Meetup/notes.txt".to_string(), format!("web:{q}")]);
    let search = |q: &str| serde_json::to_value(t.d.handle(serde_json::from_value(json!({"op": "search", "id": 1, "q": q})).unwrap())).unwrap();
    assert!(!search("golden retriever video")["rows"].to_string().contains("Documents/Videos"));
    for (q, key) in [("meetup folder", meetup), ("documents folder", "file:Documents")] {
        let v = search(q);
        assert_eq!((&v["filter"], &v["rows"][0]["key"]), (&json!(["folder"]), &json!(key)), "{q}");
    }
    // Created, moved and deleted folders come and go.
    std::thread::sleep(Duration::from_millis(500)); // the watches
    let docs = t.dir.join("home/Documents");
    std::fs::create_dir(docs.join("Tasmania")).unwrap();
    until("new folder", Duration::from_secs(2), || t.keys("tasmania")[0] == "file:Documents/Tasmania");
    std::fs::rename(docs.join("Tasmania"), docs.join("Meetup/Tassie")).unwrap();
    until("moved folder", Duration::from_secs(2), || t.keys("tassie")[0] == "file:Documents/Meetup/Tassie" && t.keys("tasmania").len() == 1);
    std::fs::remove_dir(docs.join("Meetup/Tassie")).unwrap();
    until("deleted folder", Duration::from_secs(2), || t.keys("tassie").len() == 1);
    // Enter opens the folder; Ctrl+Enter shows it in its parent. Picked under a file, it leaves the weights alone.
    std::fs::write(docs.join("meetup.txt"), "meetup plans").unwrap();
    until("a file named like it", Duration::from_secs(2), || t.keys("meetup")[0] == "file:Documents/meetup.txt");
    let w0 = *t.d.weights.read().unwrap();
    let open = |alt: bool| t.req(json!({"op": "activate", "id": 1, "key": meetup, "q": "meetup", "alt": alt}))["argv"].clone();
    assert_eq!((open(false), open(true)), (json!(["xdg-open", docs.join("Meetup")]), json!(["xdg-open", docs])));
    until("the picks", Duration::from_secs(2), || t.d.picks.read().unwrap().len() == 2);
    assert_eq!(*t.d.weights.read().unwrap(), w0);
}

/// Pictures whose "pixels" are words for the fake embedder.
fn pictures(t: &T) {
    let mut docs = Vec::new();
    for (name, words) in [("dog.jpg", "golden retriever park"), ("sunset.jpg", "golden sunset beach"), ("cat.jpg", "cat sofa photo image")] {
        let img = t.dir.join(format!("cache/{name}/f0001.jpg"));
        std::fs::create_dir_all(img.parent().unwrap()).unwrap();
        std::fs::write(&img, words).unwrap();
        let part = Part { text: String::new(), locator: Locator::default(), embed: Embed::Image(img) };
        docs.push(Doc { kind: Kind::Image, parts: vec![part], ..doc(&format!("file:Pictures/{name}"), Kind::Image, name, "", &[]) });
    }
    t.put(docs);
}

#[test]
fn picture_features_leave_files_and_apps_alone() {
    let t = start(0, "");
    corpus(&t);
    pictures(&t);
    let queries = ["term", "rent due", "retriever", "puppy plays", "shopping list", "web browser"];
    let others = |t: &T| -> Vec<Vec<(String, f32)>> {
        queries.iter().map(|q| t.search(q).0.into_iter().filter(|r| r.kind != Kind::Image).map(|r| (r.key, r.score)).collect()).collect()
    };
    // Off by default: no extra query input, no feature value, so scores are as in round 1.
    let before = others(&t);
    assert!(t.d.query_vec("golden retriever").unwrap().photo.is_none());
    for key in ["file:Pictures/dog.jpg", "file:Pictures/cat.jpg"] {
        let e = t.req(json!({"op": "explain", "id": 1, "q": "golden retriever", "key": key}));
        assert!(e["features"]["photo"].is_null() && e["features"]["generic"].is_null(), "{e}");
    }
    let asked = t.queries.load(Ordering::SeqCst);
    {
        let mut w = t.d.weights.write().unwrap();
        w[search::PHOTO] = 0.9;
        w[search::GENERIC] = -1.0;
    }
    // On: files and apps keep their order (scores may move a little through the margin feature).
    let keys = |v: &[Vec<(String, f32)>]| v.iter().map(|r| r.iter().map(|x| x.0.clone()).collect::<Vec<_>>()).collect::<Vec<_>>();
    assert_eq!(keys(&others(&t)), keys(&before));
    for q in queries {
        assert_ne!(t.search(q).0[0].kind, Kind::Image, "{q}: no picture jumps above the file or app");
    }
    let picture = t.search("golden retriever").0.into_iter().find(|r| r.kind == Kind::Image).unwrap();
    assert_eq!(picture.key, "file:Pictures/dog.jpg", "the right picture still leads the pictures");
    // "a photo of cat" matches the cat picture better than "cat" does.
    let e = t.req(json!({"op": "explain", "id": 1, "q": "cat", "key": "file:Pictures/cat.jpg"}));
    assert!(e["features"]["photo"]["value"].as_f64().unwrap() > 0.0, "{e}");
    assert!(t.d.generic.get().is_some(), "generic captions embedded once the feature is on");
    assert!(t.queries.load(Ordering::SeqCst) > asked);
}

#[test]
fn kinds_and_alt_actions() {
    let t = start(0, "");
    corpus(&t);
    let search = |q: &str, kinds: Value| {
        let v = serde_json::to_value(t.d.handle(serde_json::from_value(json!({"op": "search", "id": 1, "q": q, "kinds": kinds})).unwrap())).unwrap();
        (v["rows"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap().to_string()).collect::<Vec<_>>(), v["empty"].as_bool().unwrap())
    };
    assert_eq!(search("rent", json!(["file"])), (vec!["file".to_string()], false));
    assert_eq!(search("rent", json!(["app"])), (vec![], true));
    assert_eq!(search("rent", json!(["app", "web"])), (vec!["web".to_string()], true));
    let alt = |key: &str| t.req(json!({"op": "activate", "id": 1, "key": key, "q": "rent", "alt": true}))["argv"].clone();
    assert_eq!(alt("file:Documents/lease.pdf"), json!(["xdg-open", "/home/u/Documents"]));
    assert_eq!(alt("app:terminal"), json!(["wl-copy", "--", "terminal"]));
    assert_eq!(alt("web:rent"), json!(["wl-copy", "--", "https://duckduckgo.com/?q=rent"]));
    let answer = t.req(json!({"op": "activate", "id": 1, "key": "answer", "q": "2+3", "alt": true}));
    assert_eq!(answer["argv"], json!(["wl-copy", "--", "5"]), "Ctrl+Enter on the answer card copies too");
}

#[test]
fn a_source_whose_text_is_unchanged_still_updates_what_is_shown() {
    let t = start(0, "");
    let clip = doc("clip:1", Kind::Clip, "meetup notes", "the meetup is at the library", &[]);
    let later = Doc { subtitle: "from Chromium · 2 h ago".into(), ..clip.clone() };
    assert_eq!(ingest::source_stamp(&clip), ingest::source_stamp(&clip.clone()));
    assert_ne!(ingest::source_stamp(&clip), ingest::source_stamp(&later));
    t.put(vec![clip]);
    t.d.store.put(vec![(later.clone(), ingest::source_stamp(&later), 0)]).unwrap();
    assert_eq!(t.d.store.counts().0, 0, "nothing to embed again");
    assert_eq!(t.search("library").0[0].subtitle, "from Chromium · 2 h ago");
}

#[test]
fn hits_put_named_and_keyword_parts_first_and_keep_lyric_lines() {
    let t = start(0, "");
    let part = |text: &str, locator: Locator, embed: bool| {
        Part { embed: if embed { Embed::Text(doc_text("sales.xlsx", text)) } else { Embed::None }, text: text.into(), locator }
    };
    let sheet = |s: &str| Locator { sheet: Some(s.into()), row: Some(1), ..Locator::default() };
    let at = |a: f32, b: f32| Locator { t: Some(a), t_end: Some(b), ..Locator::default() };
    let lyrics = "you took the spare key and left the light on\nyour keys are on the counter";
    t.put(vec![
        Doc { parts: vec![
            part("toowong sales by month and many other words here about stores", sheet("Summary"), true),
            part("month coffees sold", sheet("Toowong"), true),
            part("sales month", sheet("South Bank"), true),
        ], ..doc("file:Documents/sales.xlsx", Kind::File, "sales.xlsx", "", &[]) },
        Doc { parts: vec![
            part(lyrics, Locator::default(), true),
            part("you took the spare key and left the light on", at(2.0, 10.0), false),
            part("your keys are on the counter", at(10.0, 18.0), false),
        ], ..doc("file:Music/keys.mp3", Kind::Audio, "keys.mp3", "", &[]) },
    ]);
    let row = |q: &str, key: &str| t.search(q).0.into_iter().find(|r| r.key == key).unwrap();
    let r = row("toowong sales month", "file:Documents/sales.xlsx");
    let sheets: Vec<_> = r.hits.iter().map(|h| h.locator.sheet.clone().unwrap()).collect();
    // South Bank is closest by meaning, Summary also has the words, the query names Toowong.
    assert_eq!(sheets, ["Toowong", "Summary", "South Bank"]);
    assert!(matches!(&r.action, Action::Open { locator, .. } if locator.sheet.as_deref() == Some("Toowong")));
    let r = row("spare key light", "file:Music/keys.mp3");
    assert_eq!(r.hits.iter().map(|h| h.locator.t).collect::<Vec<_>>(), [Some(2.0)], "the lyric line, though the whole lyrics matched by meaning");
    let r = row("light counter", "file:Music/keys.mp3");
    assert!(r.snippet.unwrap().text.contains("light on / your keys"), "lyric lines shown apart");
}

#[test]
fn embedding_order_follows_use_and_age_and_progress_says_so() {
    let xbel = r#"<bookmark href="file:///HOME/Documents/used%20one.txt" added="x"><bookmark href="file:///elsewhere/x.txt">"#;
    let t = start_with(0, "", Some("http://127.0.0.1:9/v1/embeddings"), |home| {
        std::fs::create_dir_all(home.join(".local/share")).unwrap();
        std::fs::write(home.join(".local/share/recently-used.xbel"), xbel.replace("/HOME", &home.display().to_string())).unwrap();
    });
    assert!(t.d.store.used.read().unwrap().contains("file:Documents/used one.txt"), "GTK's recently used files count as used");
    let one = |key: &str, kind: Kind, body: &str| doc(key, kind, body, body, &[]);
    let two = |key: &str, a: &str, b: &str| Doc { parts: [a, b].map(|x| one(key, Kind::File, x).parts.remove(0)).to_vec(), ..one(key, Kind::File, a) };
    let photo = |key: &str| {
        let part = Part { text: String::new(), locator: Locator::default(), embed: Embed::Image(t.dir.join(key)) };
        Doc { parts: vec![part], ..one(key, Kind::Image, key) }
    };
    let docs = vec![
        (one("emoji:x", Kind::Emoji, "emoji"), 0), (one("history:x", Kind::History, "history"), 0), (photo("file:Pictures/a.jpg"), 9),
        (two("file:Documents/old.txt", "old gist", "old deep"), 100), (two("file:Documents/new.txt", "new gist", "new deep"), 200),
        (two("file:Documents/used one.txt", "used gist", "used deep"), 50), (one("clip:x", Kind::Clip, "clip"), 0),
        (one("setting:x", Kind::Setting, "setting"), 0), (one("app:x", Kind::App, "app"), 0),
    ];
    t.d.store.put(docs.into_iter().map(|(d, m)| (d, "1".into(), m)).collect()).unwrap();
    let order: Vec<String> = t.d.store.pending(50).unwrap().into_iter().map(|p| p.text.split(": ").last().unwrap().to_string()).collect();
    assert_eq!(order, ["app", "setting", "used gist", "new gist", "old gist", "used deep", "clip", "new deep", "old deep", "history", "emoji", ""]);
    // Media not read yet: photos newest first, then songs, then videos.
    for (key, kind, mtime) in [("file:Documents/v.mp4", Kind::Video, 300), ("file:Documents/s.mp3", Kind::Audio, 300), ("file:Documents/p1.jpg", Kind::Image, 100), ("file:Documents/p2.jpg", Kind::Image, 200)] {
        t.d.store.put(vec![(Doc { parts: Vec::new(), ..one(key, kind, key) }, "quick".into(), mtime)]).unwrap();
        t.d.media.lock().unwrap().insert(t.dir.join("home").join(&key[5..]));
    }
    assert_eq!(ingest::next_media(&t.d).unwrap().0, t.dir.join("home/Documents/p2.jpg"));
    let v = serde_json::to_value(t.d.handle(serde_json::from_value(json!({"op": "search", "id": 1, "q": "app"})).unwrap())).unwrap();
    assert_eq!(v["progress"], json!({"done": 0, "total": 16, "stage": "apps and settings"}));
    assert_eq!(t.req(json!({"op": "status", "id": 1}))["progress"]["total"], 16);
}

#[test]
fn ready_made_vectors_are_found_by_their_exact_input() {
    let url = fake_embedder(Duration::ZERO, Arc::new(AtomicUsize::new(0)));
    let file = std::env::temp_dir().join(format!("ss-ready-{}.bin", std::process::id()));
    let texts: Vec<String> = ["title: Dog | text: pet", "title: Cat | text: pet", "title: Wi-Fi | text: connect"].map(String::from).to_vec();
    ingest::write_ready(&url, &texts, &file).unwrap();
    let data = std::fs::read(&file).unwrap();
    let _ = std::fs::remove_file(&file);
    assert_eq!(data.len(), 3 * (12 + crate::embed::DIM));
    for t in &texts {
        let ((v, s), (got, scale)) = (crate::embed::quantize(&fake_vec(t)), crate::store::lookup(&data, t).unwrap());
        assert!(got == v.iter().map(|&x| x as u8).collect::<Vec<_>>() && (scale - s).abs() < 1e-6, "{t}");
    }
    assert_eq!(crate::store::lookup(&data, "title: Dog | text: pets"), None);
    assert_eq!(crate::store::lookup(&[], "title: Dog | text: pet"), None, "an empty file has none");
}

#[test]
fn at_most_three_emoji_unless_they_are_all_or_asked_for() {
    let t = start(0, "");
    let mut docs: Vec<Doc> = (0..5).map(|i| doc(&format!("emoji:{i}"), Kind::Emoji, &format!("Dog {i}"), "dog woof", &[])).collect();
    docs.push(doc("file:Documents/dog.txt", Kind::File, "dog.txt", "walking the dog", &[]));
    t.put(docs);
    let emoji = |q: &str| t.search(q).0.iter().filter(|r| r.kind == Kind::Emoji).count();
    assert_eq!(emoji("dog"), 3);
    assert_eq!(emoji("woof"), 5, "emoji are the only matches");
    assert_eq!(emoji("emoji dog"), 5, "the query asks for emoji");
}

#[test]
fn every_matching_kind_shows_in_the_first_five_and_videos_keep_every_moment() {
    let t = start(0, "");
    // Like the Pi's "golden retriever": six dog photos, the weekend video's dog moments match less well.
    let media = |key: &str, kind: Kind, frames: &[(&str, Locator)]| {
        let parts = frames.iter().enumerate().map(|(i, (words, locator))| {
            let img = t.dir.join(format!("cache/{key}/f{:04}.jpg", i + 1));
            std::fs::create_dir_all(img.parent().unwrap()).unwrap();
            std::fs::write(&img, words).unwrap();
            Part { text: String::new(), locator: locator.clone(), embed: Embed::Image(img) }
        });
        Doc { parts: parts.collect(), ..doc(key, kind, &key[5..], "", &[]) }
    };
    let at = |a: f32, b: f32| Locator { t: Some(a), t_end: Some(b), ..Locator::default() };
    let (dog, beach, street) = ("golden retriever swimming lake water splash paddle", "waves breaking on beach", "city street traffic");
    let mut docs: Vec<Doc> = ["park", "stick", "ball", "grass", "sofa", "snow"]
        .iter().map(|w| media(&format!("file:IMG_{w}.jpg"), Kind::Image, &[(&format!("golden retriever {w}"), Locator::default())])).collect();
    docs.push(media("file:cat.jpg", Kind::Image, &[("cat on a sofa", Locator::default())]));
    docs.push(media("file:traffic.mp4", Kind::Video, &[(street, at(0.0, 8.0))]));
    docs.push(media("file:weekend.mp4", Kind::Video, &[(beach, at(0.0, 8.0)), (beach, at(8.0, 10.0)), (dog, at(10.0, 18.0)), (dog, at(18.0, 26.0)),
        (dog, at(26.0, 30.0)), (street, at(30.0, 38.0)), (street, at(38.0, 45.0)), (dog, at(45.0, 53.0)), (dog, at(53.0, 59.0))]));
    t.put(docs);
    let (rows, _) = t.search("golden retriever");
    use Kind::{Emoji as E, Image as I, Video as V};
    assert_eq!(rows.iter().map(|r| r.kind).collect::<Vec<_>>(), [I, I, I, I, V, I, I, Kind::Web], "four photos, the video, the rest; nothing below its cutoff");
    assert!(rows.iter().all(|r| r.kind != I || r.score > rows[4].score), "by score alone the video would come 7th");
    let mut moments: Vec<_> = rows[4].hits.iter().map(|h| (h.locator.t, h.locator.t_end)).collect();
    moments.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(moments, [(Some(10.0), Some(30.0)), (Some(45.0), Some(59.0))], "every frame above the cutoff, neighbours merged");
    let (rows, _) = t.search("puppy");
    assert!(rows.len() == 7 && rows[..6].iter().all(|r| r.kind == I) && rows.windows(2).all(|w| w[0].score >= w[1].score), "one kind: by score");
    t.put((0..5).map(|i| doc(&format!("emoji:{i}"), Kind::Emoji, &format!("Golden retriever {i}"), "", &[])).collect());
    let kinds: Vec<Kind> = t.search("golden retriever").0.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [E, E, E, I, I, I, I, V, I, I, Kind::Web], "only the emoji shown count");
}

#[test]
fn media_hits_are_sections_that_take_in_moments_just_under_the_cutoff() {
    let t = start(0, "");
    // "golden retriever" is 0.535 from a frame with five more words, 0.5 from one with six, 0 from the beach.
    std::fs::write(t.d.cfg.prefs(), FAKE_CUTOFFS.replace("frame = 0.5", "frame = 0.51")).unwrap();
    t.d.reload_prefs();
    let at = |a: f32, b: f32| Locator { t: Some(a), t_end: Some(b), ..Locator::default() };
    let frame = |i: usize, words: &str, loc: Locator| {
        let img = t.dir.join(format!("cache/v/f{i:04}.jpg"));
        std::fs::create_dir_all(img.parent().unwrap()).unwrap();
        std::fs::write(&img, words).unwrap();
        Part { text: String::new(), locator: loc, embed: Embed::Image(img) }
    };
    let (best, near, beach) = ("golden retriever a1 a2 a3 a4 a5", "golden retriever b1 b2 b3 b4 b5 b6", "waves breaking on beach");
    let frames = [(beach, at(0.0, 10.0)), (near, at(10.0, 20.0)), (near, at(20.0, 30.0)), (beach, at(30.0, 45.0)), (best, at(45.0, 59.0))];
    let window = |words: &str, loc: Locator| Part { embed: Embed::Text(doc_text("memo", words)), text: words.into(), locator: loc };
    t.put(vec![
        Doc { parts: frames.iter().enumerate().map(|(i, (w, l))| frame(i, w, l.clone())).collect(), ..doc("file:v.mp4", Kind::Video, "v.mp4", "", &[]) },
        Doc { parts: vec![window("sponsor gives six hundred dollars", at(0.0, 13.0)), window("venue costs five hundred", at(13.0, 27.0)),
            window("venue costs include projector", at(27.0, 38.0)), window("library free room backup", at(38.0, 50.0))],
            action: Action::Open { path: "/home/u/memo.m4a".into(), locator: Locator { t_end: Some(55.0), ..Locator::default() } },
            ..doc("file:memo.m4a", Kind::Audio, "memo.m4a", "", &[]) },
    ]);
    let row = |q: &str| t.search(q).0.into_iter().find(|r| r.kind != Kind::Web).unwrap();
    let sections = |r: &Row| r.hits.iter().map(|h| (h.locator.t.unwrap(), h.locator.t_end.unwrap())).collect::<Vec<_>>();
    let v = row("golden retriever");
    assert_eq!((sections(&v), v.duration), (vec![(45.0, 59.0), (10.0, 30.0)], Some(59.0)), "best first; the near frames, just under the cutoff, too");
    let m = row("venue costs");
    assert_eq!((sections(&m), m.duration), (vec![(13.0, 38.0)], Some(55.0)), "neighbouring windows as one section; the file's length, past the speech");
    assert!(m.hits[0].text.starts_with("venue costs"));
}

#[test]
fn type_words_filter_by_kind_unless_they_name_what_the_filter_hides() {
    let words = |q: &str| search::type_words(q);
    let s = |q: &str, k: &[Kind]| Some((q.to_string(), k.to_vec(), false, false));
    assert_eq!(words("golden retriever video"), s("golden retriever", &[Kind::Video]));
    assert_eq!(words("only golden retriever video"), s("golden retriever", &[Kind::Video]));
    assert_eq!(words("Golden Retriever audio and text"), s("golden retriever", &[Kind::Audio, Kind::File]));
    assert_eq!(words("dog photos, films & voice memos"), s("dog", &[Kind::Image, Kind::Video, Kind::Audio]));
    assert_eq!(words("lease pdf only"), Some(("lease".into(), vec![Kind::File], true, false)), "PDFs, not any file");
    assert_eq!(words("invoice memo pdf"), Some(("invoice".into(), vec![Kind::Audio, Kind::File], true, false)), "voice memo is two words");
    assert_eq!(words("lease pdfs or docs"), s("lease", &[Kind::File]), "docs are any file");
    assert_eq!(words("awkward emoji"), s("awkward", &[Kind::Emoji]));
    assert_eq!(words("voice memos folder"), s("voice memos", &[Kind::Folder]), "type words name a folder");
    for q in ["text editor", "photo viewer", "music player", "app idea for watering plants", "website redesign invoice", "screenshot",
        "photos and videos", "dog clip", "lease page", "rock and roll", "folder"] {
        assert_eq!(words(q), None, "{q}");
    }
    let t = start(0, "[[boost]]\nkey = \"app:puppy\"\nby = 1.0\n");
    corpus(&t);
    t.put(vec![
        doc("clip:1", Kind::Clip, "my puppy", "my puppy at the park", &[]),
        doc("app:puppy", Kind::App, "Puppy", "a pet game", &[]),
        doc("setting:screenshot", Kind::Setting, "Screenshot", "capture the screen", &["take a screenshot"]),
        doc("setting:hardware", Kind::Setting, "Hardware", "bluetooth audio and printers", &[]),
        doc("setting:spotify", Kind::Setting, "Spotify", "launch the player", &[]),
        doc("setting:window-pop", Kind::Setting, "Pop a window out onto a display", "float it", &[]),
        doc("setting:monitors", Kind::Setting, "Monitors", "resolution and scale", &["display settings"]),
        doc("setting:network", Kind::Setting, "Network", "wireless connections", &["wifi"]),
        doc("setting:audio", Kind::Setting, "Audio", "volume and outputs", &[]),
        doc("app:settings", Kind::App, "Settings Manager", "configure the desktop", &[]),
        doc("emoji:🎶", Kind::Emoji, "Musical notes", "music", &[]),
        doc("file:Documents/lease-notes.md", Kind::File, "lease-notes.md", "questions about the lease", &[]),
        doc("file:Documents/movies-to-watch.md", Kind::File, "movies-to-watch.md", "Dune, Arrival", &[]),
    ]);
    let search = |q: &str, kinds: Value| {
        let v = serde_json::to_value(t.d.handle(serde_json::from_value(json!({"op": "search", "id": 1, "q": q, "kinds": kinds})).unwrap())).unwrap();
        (v["filter"].clone(), v["rows"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_string()).collect::<Vec<_>>())
    };
    let keys = |k: &[&str]| k.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    assert_eq!(search("puppy text", Value::Null), (json!(["file"]), keys(&["file:Documents/puppy.txt", "web:puppy text"])), "an app named puppy, but a file matches");
    assert_eq!(search("puppy text", json!(["clip", "file"])).0, Value::Null, "kinds the request gives win");
    assert_eq!(search("zebra text", Value::Null), (json!(["file"]), keys(&["web:zebra text"])), "nothing else matches zebra either");
    let (filter, rows) = search("take a screenshot", Value::Null);
    assert_eq!((filter, rows[0].as_str()), (Value::Null, "setting:screenshot"), "the setting's name, not a search for pictures");
    assert_eq!(search("bluetooth audio", Value::Null).0, Value::Null, "every word of a setting");
    let (filter, rows) = search("musical notes", Value::Null);
    assert_eq!((filter, rows[0].as_str()), (Value::Null, "emoji:🎶"), "an emoji's name");
    let (filter, rows) = search("spotify music", Value::Null);
    assert_eq!((filter, rows[0].as_str()), (Value::Null, "setting:spotify"), "no song matches, the Spotify setting does");
    let (filter, rows) = search("display settings", Value::Null);
    assert_eq!((filter, rows[0].as_str()), (json!(["setting"]), "setting:monitors"), "a setting's name holds its type word");
    assert_eq!(search("lease pdf", Value::Null), (json!(["file"]), keys(&["file:Documents/lease.pdf", "web:lease pdf"])), "PDFs only");
    assert_eq!(search("apps firefox", Value::Null), (json!(["app"]), keys(&["app:firefox", "web:apps firefox"])));
    assert_eq!(search("settings wifi", Value::Null), (json!(["setting"]), keys(&["setting:network", "web:settings wifi"])), "named wifi");
    assert_eq!(search("settings audio", Value::Null), (json!(["setting"]), keys(&["setting:audio", "setting:hardware", "web:settings audio"])), "named audio");
    for (q, key) in [("settings manager", "app:settings"), ("settings manag", "app:settings"), ("movies to watch", "file:Documents/movies-to-watch.md")] {
        let (filter, rows) = search(q, Value::Null);
        assert_eq!((filter, rows[0].as_str()), (Value::Null, key), "{q}: the start of a name the filter would hide");
    }
    // Learning counts only the rows the filter showed: the boosted Puppy app ranks above puppy.txt, but only unfiltered.
    let w0 = *t.d.weights.read().unwrap();
    let picked = |n: usize| until("the pick is learnt", Duration::from_secs(2), || t.d.picks.read().unwrap().len() == n);
    let open = t.req(json!({"op": "activate", "id": 1, "key": "file:Documents/puppy.txt", "q": "puppy text"}));
    assert_eq!(open["argv"][1], "/home/u/Documents/puppy.txt");
    picked(1);
    assert_eq!(*t.d.weights.read().unwrap(), w0, "first of the rows shown");
    t.req(json!({"op": "activate", "id": 2, "key": "file:Documents/puppy.txt", "q": "puppy"}));
    picked(2);
    assert_ne!(*t.d.weights.read().unwrap(), w0, "under the app");
}

#[test]
fn leading_plural_type_words_scope_the_search_and_singular_ones_stay_whole() {
    let led = |q: &str, k: &[Kind], pdf: bool| Some((q.to_string(), k.to_vec(), pdf, true));
    for (q, k) in [("apps firefox", Kind::App), ("settings wifi", Kind::Setting), ("files lease", Kind::File), ("docs lease", Kind::File),
        ("photos beach", Kind::Image), ("videos dog", Kind::Video), ("songs breakup", Kind::Audio), ("voice memos groceries", Kind::Audio),
        ("emojis happy", Kind::Emoji), ("emoji money", Kind::Emoji), ("clipboard lease", Kind::Clip), ("websites github", Kind::History),
        ("settings audio", Kind::Setting), ("apps files", Kind::App), ("folder camera", Kind::Folder), ("dir camera", Kind::Folder),
        ("directory camera", Kind::Folder), ("dirs camera", Kind::Folder)] {
        assert_eq!(search::type_words(q), led(q.rsplit(' ').next().unwrap(), &[k], false), "{q}");
    }
    assert_eq!(search::type_words("pdfs only lease"), led("lease", &[Kind::File], true));
    assert_eq!(search::type_words("apps and settings wifi"), led("wifi", &[Kind::App, Kind::Setting], false));
    assert_eq!(search::type_words("songs about a breakup"), led("about a breakup", &[Kind::Audio], false));
    assert_eq!(search::type_words("apps text editor"), led("text editor", &[Kind::App], false), "up to the singular word");
    for q in ["photo editor", "video call", "image viewer", "app idea", "history essay", "music festival tickets", "audio mixer", "footage review"] {
        assert_eq!(search::type_words(q), None, "{q}: singular, or nothing else left");
    }
}

#[test]
fn at_most_two_history_rows_unless_asked_for() {
    let t = start(0, "");
    let mut docs: Vec<Doc> = (0..5).map(|i| doc(&format!("history:{i}"), Kind::History, &format!("Dog news {i}"), "https://dog.com/news dog", &[])).collect();
    docs.push(doc("file:Documents/dog.txt", Kind::File, "dog.txt", "dog.com walking the dog", &[]));
    t.put(docs);
    let history = |q: &str, kinds: Option<&[Kind]>| search::search(&t.d, q, 30, t.d.query_vec(q).as_ref(), kinds, false).0.iter().filter(|r| r.kind == Kind::History).count();
    assert_eq!(history("dog", None), 2);
    assert_eq!(history("dog.com", None), 5, "a web address asks for history");
    assert_eq!(history("dog", Some(&[Kind::File, Kind::History])), 5, "so do kinds");
}

#[test]
fn a_title_word_with_one_typo_is_a_name_match() {
    let t = start(0, "");
    corpus(&t);
    t.put(vec![
        doc("app:chromium", Kind::App, "Chromium", "Browse the web", &[]),
        doc("file:Documents/chrome-notes.txt", Kind::File, "chrome-notes.txt", "tabs", &[]),
    ]);
    // Names only (no query vector), so only the typo rule can match.
    let names = |q: &str| search::search(&t.d, q, 30, None, None, false);
    for q in ["chromum", "chormium", "chromiun", "Chromiumm"] {
        let (rows, _) = names(q);
        assert_eq!((rows[0].key.as_str(), rows[0].why, rows.len()), ("app:chromium", Why::Name, 2), "{q}");
    }
    assert_eq!(t.keys("chromum")[0], "app:chromium", "first with meaning too");
    assert_eq!(names("termnal").0[0].key, "app:terminal");
    for q in ["chrmiun", "chromum tabs", "quantum chromodynamics lecture notes", "lease.pdq"] {
        assert!(names(q).1, "{q}: two edits, a word not in the title, or a word under 5 letters with a typo");
    }
}

#[test]
fn under_an_answer_card_near_misses_by_meaning_go() {
    let t = start(0, "");
    t.put(vec![
        doc("file:Documents/flight.txt", Kind::File, "flight.txt", "sydney", &[]),
        doc("file:Documents/trip.txt", Kind::File, "trip.txt", "sydney jakarta", &[]),
    ]);
    // flight.txt is 0.408 from the query: above the file cutoff, by less than 0.03.
    std::fs::write(t.d.cfg.prefs(), FAKE_CUTOFFS.replace("file = 0.3", "file = 0.39")).unwrap();
    t.d.reload_prefs();
    let q = "3pm sydney in jakarta";
    assert!(ss_answers::answer(q, None, crate::store::now(), &t.d.tz).is_some());
    assert_eq!(t.keys(q), ["file:Documents/trip.txt", &format!("web:{q}")]);
    let e = t.req(json!({"op": "explain", "id": 1, "q": q, "key": "file:Documents/flight.txt"}));
    assert_eq!((e["kept"].as_bool(), e["rank"].is_null()), (Some(true), true), "a match, not shown under the answer");
}

/// A daemon with no threads, its servers down, in a fresh folder.
fn bare() -> Arc<Daemon> {
    let dir = std::env::temp_dir().join(format!("ss{}-bare-{:?}", std::process::id(), std::thread::current().id()));
    let down = "http://127.0.0.1:9/v1/embeddings".to_string();
    let cfg = Cfg { home: dir.join("home"), state: dir.join("state"), cache: dir.join("cache"), config: dir.join("config"),
        sock: dir.join("s.sock"), query_url: down.clone(), index_url: down };
    let _ = std::fs::remove_dir_all(&dir);
    Daemon::open(cfg).unwrap()
}

#[test]
fn a_query_server_that_is_down_never_says_nothing_matches() {
    let d = bare();
    d.store.put(vec![(doc("file:Documents/puppy.txt", Kind::File, "puppy.txt", "my puppy plays fetch", &[]), "1".into(), 0)]).unwrap();
    let final_phase = |q: &str| match d.search_response(&server::Search::new(&d, 1, q.into(), 30, None), Phase::Final, 0.0) {
        Response::Search { rows, empty, .. } => (rows.len(), empty),
        _ => unreachable!(),
    };
    assert_eq!(final_phase("retriever"), (1, false), "only the web row, but no claim that nothing matches");
    assert_eq!(final_phase("zx"), (1, true), "two letters never ask for meaning");
}

#[test]
fn media_deleted_while_it_is_read_leaves_no_row() {
    let t = start(0, "");
    std::thread::sleep(Duration::from_millis(500));
    let song = t.dir.join("home/Documents/slow.mp3");
    std::fs::write(song.with_extension("lrc"), "words of a slow song").unwrap();
    std::fs::write(&song, "audio").unwrap();
    until("being read", Duration::from_secs(3), || t.d.heavy.try_lock().is_err());
    std::fs::remove_file(&song).unwrap();
    until("forgotten", Duration::from_secs(3), || ext::FORGOT.lock().unwrap().contains(&song));
    until("read to the end", Duration::from_secs(3), || t.d.heavy.try_lock().is_ok());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(t.d.store.stamp("file:Documents/slow.mp3"), None, "no ghost row");
    assert!(t.d.media.lock().unwrap().is_empty());
}

#[test]
fn excluded_files_are_forgotten_and_never_read() {
    let t = start_with(0, "", None, |home| {
        std::fs::create_dir_all(home.join("Documents/private")).unwrap();
        std::fs::write(home.join("Documents/private/pins.txt"), "vault code 1234").unwrap();
        std::fs::write(home.join("Documents/open.txt"), "vault notes").unwrap();
    });
    let (private, open) = ("file:Documents/private/pins.txt", "file:Documents/open.txt");
    assert!(t.d.store.stamp(private).is_some());
    t.req(json!({"op": "pref", "id": 1, "change": {"exclude": {"path": "~/Documents/private"}}}));
    until("excluded folder forgotten", Duration::from_secs(3), || t.d.store.stamp(private).is_none());
    t.req(json!({"op": "pref", "id": 1, "change": {"exclude": {"key": open}}}));
    for op in [json!({"op": "read", "id": 1, "key": open}), json!({"op": "explain", "id": 1, "q": "vault", "key": open}), json!({"op": "similar", "id": 1, "key": open})] {
        match t.d.handle(serde_json::from_value(op).unwrap()) {
            Response::Error { error, .. } => assert_eq!(error, format!("{open} is not indexed")),
            other => panic!("{}", serde_json::to_string(&other).unwrap()),
        }
    }
}

#[test]
fn enter_runs_the_action_before_learning_from_it() {
    let t = start(300, "");
    corpus(&t);
    let t0 = Instant::now();
    let v = t.req(json!({"op": "activate", "id": 1, "key": "app:firefox", "q": "web browsing"}));
    assert!(t0.elapsed() < Duration::from_millis(250), "waited {:?} for the query server", t0.elapsed());
    assert_eq!(v["argv"], json!(["gtk-launch", "firefox"]));
    until("pick learned", Duration::from_secs(3), || t.d.picks.read().unwrap().iter().any(|p| p.key == "app:firefox" && p.q == "web browsing"));
}

#[test]
fn the_watcher_skips_hidden_paths_and_folders_in_home() {
    let home = std::path::Path::new("/h");
    let roots = [home.join("Documents"), home.to_path_buf()];
    let w = |p: &str| ingest::watched(home, &roots, &home.join(p));
    assert!(w("Documents/a/b.txt") && w("notes.txt") && w("Documents"));
    for p in [".local/state/superspace/index.sqlite-wal", ".config/chromium/Default/Cookies", "Documents/.git/index", "Other/x.txt"] {
        assert!(!w(p), "{p}");
    }
}

#[test]
fn a_second_daemon_refuses_before_opening_the_index() {
    let d = bare();
    let cfg = Cfg { state: d.cfg.state.with_file_name("state2"), ..d.cfg.clone() };
    let held = server::lock(&cfg.state).unwrap();
    let e = server::serve(cfg.clone()).unwrap_err();
    assert!(format!("{e:#}").contains("already serving"), "{e:#}");
    assert!(!cfg.state.join("index.sqlite").exists(), "the index is not touched");
    drop(held);
    // A child another test is starting may hold a copy of the lock until it execs.
    until("lock released", Duration::from_secs(1), || server::lock(&cfg.state).is_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn indexing_threads_run_at_nice_19_and_searches_do_not() {
    let _t = start(0, "");
    let nice = |stat: &std::path::Path| std::fs::read_to_string(stat).unwrap().rsplit(')').next().unwrap().split_whitespace().nth(16).unwrap().parse::<i32>().unwrap();
    let low = std::fs::read_dir("/proc/self/task").unwrap().flatten().filter(|e| nice(&e.path().join("stat")) == 19).count();
    assert!(low >= 4, "{low} threads at nice 19");
    assert_eq!(nice(std::path::Path::new("/proc/thread-self/stat")), 0, "only the indexing threads");
}
