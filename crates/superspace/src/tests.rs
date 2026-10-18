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

/// Stand-ins for the other crates (still stubs while they are being built).
pub mod ext {
    use ss_core::*;
    use std::path::{Path, PathBuf};
    pub fn roots(home: &Path) -> Vec<PathBuf> {
        vec![home.join("Documents")]
    }
    pub fn wanted(p: &Path) -> bool {
        p.exists() && !p.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    }
    pub fn crawl(home: &Path) -> Vec<PathBuf> {
        fn walk(d: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out) } else if wanted(&p) { out.push(p) }
            }
        }
        let mut out = Vec::new();
        walk(&home.join("Documents"), &mut out);
        out
    }
    pub fn file_doc(p: &Path, _home: &Path) -> Option<Doc> {
        let text = std::fs::read_to_string(p).ok()?;
        let name = p.file_name()?.to_string_lossy().into_owned();
        Some(Doc {
            key: String::new(),
            kind: Kind::File,
            title: name.clone(),
            subtitle: "~/Documents".into(),
            icon: "text-x-generic".into(),
            action: Action::Open { path: p.display().to_string(), locator: Locator::default() },
            stamp: String::new(),
            names: Vec::new(),
            parts: vec![Part { embed: Embed::Text(doc_text(&name, &text)), text, locator: Locator { line: Some(1), ..Locator::default() } }],
        })
    }
    pub fn is_media(_: &Path) -> bool {
        false
    }
    pub fn media_doc(_: &Path, _: &Path, _: &Path) -> Option<Doc> {
        None
    }
    pub fn apps() -> Vec<Doc> {
        Vec::new()
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
/// wait `delay`; any input containing REFUSE fails the whole request.
fn fake_embedder(delay: Duration) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/embeddings", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || serve_fake(s, delay));
        }
    });
    url
}

fn serve_fake(mut s: TcpStream, delay: Duration) {
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
    let inputs: Vec<String> = v["input"].as_array().into_iter().flatten().map(|x| x.as_str().unwrap_or("image").to_string()).collect();
    if inputs.iter().any(|t| t.starts_with("task: search result")) {
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

const FAKE_CUTOFFS: &str = "[cutoffs]\napp = 0.3\nsetting = 0.3\nfile = 0.3\nclip = 0.3\nemoji = 0.3\nhistory = 0.3\n";

struct T {
    d: Arc<Daemon>,
    dir: PathBuf,
}

fn start(delay_ms: u64, prefs: &str) -> T {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("ss{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home/Documents")).unwrap();
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("config/preferences.toml"), format!("{prefs}\n{FAKE_CUTOFFS}")).unwrap();
    let url = fake_embedder(Duration::from_millis(delay_ms));
    let cfg = Cfg {
        home: dir.join("home"),
        state: dir.join("state"),
        cache: dir.join("cache"),
        config: dir.join("config"),
        sock: dir.join("s.sock"),
        query_url: url.clone(),
        index_url: url,
    };
    let d = Daemon::open(cfg).unwrap();
    let l = server::bind(&d.cfg.sock).unwrap();
    ingest::start(&d);
    let d2 = d.clone();
    std::thread::spawn(move || server::listen(d2, l));
    let idle = || {
        let q = d.fast.lock().unwrap();
        !q.rescan && !q.sources && q.paths.is_empty() && !d.busy.load(Ordering::SeqCst)
    };
    until("first scan", Duration::from_secs(5), idle);
    T { d, dir }
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
    fn put(&self, docs: Vec<Doc>) {
        self.d.store.put(docs.into_iter().map(|d| (d, "1".into(), 0)).collect()).unwrap();
        self.d.wake_embedder();
        until("embedding", Duration::from_secs(5), || self.d.store.counts().0 == 0);
    }
    fn search(&self, q: &str) -> (Vec<Row>, bool) {
        search::search(&self.d, q, 30, self.d.query_vec(q).as_ref())
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
    let (rows, empty) = search::search(&t.d, "rent due", 30, None);
    assert!(!empty);
    assert_eq!((rows[0].key.as_str(), rows[0].why), ("file:Documents/lease.pdf", Why::Keyword));
    assert_eq!(t.keys("rent due")[0], "file:Documents/lease.pdf");
    let sn = rows[0].snippet.as_ref().unwrap();
    let marked: Vec<String> = sn.marks.iter().map(|&(a, b)| sn.text.chars().skip(a).take(b - a).collect()).collect();
    assert_eq!(marked, ["rent", "due"]);
    // Meaning only: no shared word, so the instant phase has nothing local.
    let (rows, empty) = search::search(&t.d, "retriever", 30, None);
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
    assert_eq!(t.keys("te")[0], picked, "the pick wins its prefix");
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
    assert_eq!(got[0]["value"]["items"], 5);
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
    let has = |t: &T| search::search(&t.d, "zebra crossing", 30, None).0.iter().any(|r| r.key == key);
    let added = until("new file", Duration::from_secs(2), || has(&t));
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
    assert_eq!(e["why"], "keyword");
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
    assert!(s.text.starts_with("word") && s.text.chars().count() <= 160);
    assert!(search::snippet("übergolden", &["golden".into()]).marks.is_empty());
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
