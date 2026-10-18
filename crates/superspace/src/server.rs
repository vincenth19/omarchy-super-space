//! The daemon: shared state, the socket server (newline JSON, two responses
//! per search) and the action runner.

use crate::Cfg;
use crate::embed::{self, Input, normalize, quantize};
use crate::ingest::{self, FastQ};
use crate::prefs::{self, Prefs};
use crate::search::{self, NF, QVec, norm};
use crate::store::{Pick, Store, kind_name, now};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use ss_core::{Action, Kind, Phase, Request, Response, Timing};
use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant};

pub struct Daemon {
    pub cfg: Cfg,
    pub store: Store,
    pub prefs: RwLock<Prefs>,
    pub prefs_error: Mutex<Option<String>>,
    /// Embedded `prefer` sentences.
    pub prefer: RwLock<Vec<QVec>>,
    pub picks: RwLock<Vec<Pick>>,
    pub weights: RwLock<[f32; NF]>,
    pub rates: RwLock<Option<ss_answers::Rates>>,
    qcache: Mutex<HashMap<String, QVec>>,
    /// Folders to index, deepest first.
    pub roots: Vec<PathBuf>,
    pub tz: String,
    pub fast: Mutex<FastQ>,
    pub fast_cv: Condvar,
    pub busy: AtomicBool,
    pub media: Mutex<BTreeSet<PathBuf>>,
    pub media_cv: Condvar,
    pub embed_cv: (Mutex<()>, Condvar),
}

impl Daemon {
    /// Open the index and preferences; no threads yet (see `ingest::start`).
    pub fn open(cfg: Cfg) -> Result<Arc<Daemon>> {
        let store = Store::open(&cfg.state.join("index.sqlite"))?;
        let (prefs, err) = match prefs::load(&cfg.prefs()) {
            Ok(p) => (p, None),
            Err(e) => (Prefs::default(), Some(format!("{e:#}"))),
        };
        let mut roots = ingest::ext::roots(&cfg.home);
        roots.sort_by_key(|r| std::cmp::Reverse(r.components().count()));
        let d = Arc::new(Daemon {
            picks: RwLock::new(store.picks()?),
            weights: RwLock::new(search::load_weights(&cfg.state.join("weights.json"))),
            store,
            prefs: RwLock::new(Prefs::default()),
            prefs_error: Mutex::new(err),
            prefer: RwLock::new(Vec::new()),
            rates: RwLock::new(None),
            qcache: Mutex::new(HashMap::new()),
            roots,
            tz: home_tz(),
            fast: Mutex::new(FastQ::default()),
            fast_cv: Condvar::new(),
            busy: AtomicBool::new(false),
            media: Mutex::new(BTreeSet::new()),
            media_cv: Condvar::new(),
            embed_cv: (Mutex::new(()), Condvar::new()),
            cfg,
        });
        d.set_prefs(prefs);
        Ok(d)
    }

    pub fn wake_embedder(&self) {
        self.embed_cv.1.notify_all();
    }

    /// The query's vector from the query server (cached; None when it is down,
    /// and for one or two letters, whose meaning is noise).
    pub fn query_vec(&self, q: &str) -> Option<QVec> {
        let qn = norm(q);
        if qn.chars().filter(|c| !c.is_whitespace()).count() < 3 {
            return None;
        }
        if let Some(v) = self.qcache.lock().unwrap().get(&qn) {
            return Some(v.clone());
        }
        let text = ss_core::query_text(&qn.chars().take(1000).collect::<String>());
        let v = embed::embed(&self.cfg.query_url, &[Input::Text(&text)], Duration::from_secs(5)).ok()?.pop()?;
        let qv = quantize(&v);
        let mut c = self.qcache.lock().unwrap();
        if c.len() > 500 {
            c.clear();
        }
        c.insert(qn, qv.clone());
        Some(qv)
    }

    pub fn search_response(&self, id: u64, q: &str, limit: usize, phase: Phase, t0: Instant, instant_ms: f32) -> Response {
        let (qv, embed_ms) = match phase {
            Phase::Instant => (None, 0.0),
            Phase::Final => {
                let t = Instant::now();
                (self.query_vec(q), t.elapsed().as_secs_f32() * 1000.0)
            }
        };
        let (rows, empty) = search::search(self, q, limit, qv.as_ref());
        let answer = ss_answers::answer(q, self.rates.read().unwrap().as_ref(), now(), &self.tz);
        let total_ms = t0.elapsed().as_secs_f32() * 1000.0;
        let instant_ms = if phase == Phase::Instant { total_ms } else { instant_ms };
        Response::Search { id, phase, rows, answer, empty, timing: Timing { instant_ms, embed_ms, total_ms } }
    }

    /// Every request except the two phases of a search on a connection.
    pub fn handle(&self, req: Request) -> Response {
        let (id, out) = match req {
            Request::Search { id, q, limit } => return self.search_response(id, &q, limit, Phase::Final, Instant::now(), 0.0),
            Request::Activate { id, key, q, hit } => (id, self.activate(&key, &q, hit)),
            Request::Read { id, key, part } => (id, search::read(self, &key, part)),
            Request::Similar { id, key, limit } => (id, search::similar(self, &key, limit).map(|rows| json!({ "rows": rows }))),
            Request::Explain { id, q, key } => (id, search::explain(self, &q, &key)),
            Request::Prefs { id } => (id, Ok(serde_json::to_value(&*self.prefs.read().unwrap()).unwrap_or_default())),
            Request::Pref { id, change } => (id, self.pref(&change)),
            Request::Status { id } => (id, Ok(self.status())),
        };
        match out {
            Ok(value) => Response::Value { id, value },
            Err(e) => Response::Error { id, error: format!("{e:#}") },
        }
    }

    fn activate(&self, key: &str, q: &str, hit: Option<usize>) -> Result<Value> {
        let (action, kind) = if key == "answer" {
            let a = ss_answers::answer(q, self.rates.read().unwrap().as_ref(), now(), &self.tz).context("no answer for this query")?;
            (Action::Copy { text: a.copy }, Kind::Web)
        } else if let Some(wq) = key.strip_prefix("web:") {
            (search::web_row(wq).action, Kind::Web)
        } else {
            let qv = self.query_vec(q);
            let row = search::pick(self, q, key, qv.as_ref())?;
            let mut action = row.action;
            if let (Action::Open { locator, .. }, Some(h)) = (&mut action, hit.and_then(|i| row.hits.get(i))) {
                *locator = h.locator.clone();
            }
            let p = Pick { key: key.to_string(), q: norm(q), ts: now() };
            self.store.add_pick(&p)?;
            self.picks.write().unwrap().push(p);
            (action, row.kind)
        };
        let argv = command(&action, kind);
        spawn(&argv)?;
        Ok(json!({ "action": action, "argv": argv }))
    }

    fn pref(&self, change: &Value) -> Result<Value> {
        let new = prefs::apply(&self.prefs.read().unwrap(), change)?;
        prefs::save(&self.cfg.prefs(), &new)?;
        self.set_prefs(new);
        Ok(serde_json::to_value(&*self.prefs.read().unwrap())?)
    }

    /// Re-read preferences.toml (the watcher calls this on every change).
    pub fn reload_prefs(&self) {
        match prefs::load(&self.cfg.prefs()) {
            Ok(p) => {
                *self.prefs_error.lock().unwrap() = None;
                self.set_prefs(p);
            }
            Err(e) => *self.prefs_error.lock().unwrap() = Some(format!("{e:#}")),
        }
    }

    fn set_prefs(&self, new: Prefs) {
        let old = std::mem::replace(&mut *self.prefs.write().unwrap(), new.clone());
        if old.exclude != new.exclude {
            let mut q = self.fast.lock().unwrap();
            q.rescan = true;
            q.sources = true;
            self.fast_cv.notify_all();
        }
        // Each sentence is embedded once (cached in the index).
        let mut vecs = Vec::new();
        for s in &new.prefer {
            let v = self.store.prefer_vec(s).or_else(|| {
                let text = ss_core::query_text(s);
                let v = embed::embed(&self.cfg.query_url, &[Input::Text(&text)], Duration::from_secs(10)).ok()?.pop()?;
                self.store.set_prefer_vec(s, &v).ok();
                Some(v)
            });
            vecs.extend(v.map(|v| quantize(&normalize(v))));
        }
        *self.prefer.write().unwrap() = vecs;
    }

    pub fn status(&self) -> Value {
        let (pending, pending_text, refused) = self.store.counts();
        let mem = self.store.mem.read().unwrap();
        let mut kinds: HashMap<String, usize> = HashMap::new();
        for it in mem.items.values() {
            *kinds.entry(kind_name(it.kind)).or_default() += 1;
        }
        let rss_mb = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
            let kb: f64 = s.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()?;
            Some((kb / 1024.0 * 10.0).round() / 10.0)
        });
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "items": mem.items.len(), "parts": mem.parts.len(), "vectors": mem.rows.len(),
            "pending": pending, "pending_text": pending_text, "refused": refused, "kinds": kinds,
            "indexing": self.busy.load(Ordering::Relaxed), "media_queue": self.media.lock().unwrap().len(),
            "query_server": embed::healthy(&self.cfg.query_url), "index_server": embed::healthy(&self.cfg.index_url),
            "rss_mb": rss_mb, "prefs_error": *self.prefs_error.lock().unwrap(),
            "home": self.cfg.home, "state": self.cfg.state, "socket": self.cfg.sock, "preferences": self.cfg.prefs(),
            "roots": self.roots, "time_zone": self.tz, "rates": self.rates.read().unwrap().as_ref().map(|r| r.date.clone()),
        })
    }
}

/// Home time zone: $TZ, else the /etc/localtime link target, else UTC.
fn home_tz() -> String {
    std::env::var("TZ")
        .ok()
        .map(|t| t.trim_start_matches(':').to_string())
        .filter(|t| t.contains('/'))
        .or_else(|| {
            let link = std::fs::read_link("/etc/localtime").ok()?;
            let s = link.to_string_lossy();
            Some(s[s.find("zoneinfo/")? + 9..].to_string())
        })
        .unwrap_or_else(|| "UTC".into())
}

pub fn serve(cfg: Cfg) -> Result<()> {
    let d = Daemon::open(cfg)?;
    let listener = bind(&d.cfg.sock)?;
    eprintln!("superspace: {} items, listening on {}", d.store.mem.read().unwrap().items.len(), d.cfg.sock.display());
    ingest::start(&d);
    listen(d, listener);
    Ok(())
}

pub fn bind(sock: &std::path::Path) -> Result<UnixListener> {
    if UnixStream::connect(sock).is_ok() {
        bail!("another superspace is already serving {}", sock.display());
    }
    let _ = std::fs::remove_file(sock);
    if let Some(dir) = sock.parent() {
        std::fs::create_dir_all(dir)?;
    }
    UnixListener::bind(sock).with_context(|| format!("{}", sock.display()))
}

pub fn listen(d: Arc<Daemon>, listener: UnixListener) {
    for conn in listener.incoming().flatten() {
        let d = d.clone();
        std::thread::spawn(move || connection(d, conn));
    }
}

fn send(out: &Mutex<UnixStream>, r: &Response) {
    let mut line = serde_json::to_vec(r).unwrap_or_default();
    line.push(b'\n');
    let _ = out.lock().unwrap().write_all(&line);
}

/// One client. Searches answer at once with the instant phase; the final
/// phase runs on a second thread that only ever works on the newest search.
fn connection(d: Arc<Daemon>, conn: UnixStream) {
    let Ok(w) = conn.try_clone() else { return };
    let out = Arc::new(Mutex::new(w));
    let latest = Arc::new(AtomicU64::new(u64::MAX));
    let (tx, rx) = mpsc::channel::<(u64, String, usize, Instant, f32)>();
    {
        let (d, out, latest) = (d.clone(), out.clone(), latest.clone());
        std::thread::spawn(move || {
            while let Ok(mut job) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    job = newer;
                }
                let (id, q, limit, t0, instant_ms) = job;
                if id == latest.load(Ordering::SeqCst) {
                    let r = d.search_response(id, &q, limit, Phase::Final, t0, instant_ms);
                    if id == latest.load(Ordering::SeqCst) {
                        send(&out, &r);
                    }
                }
            }
        });
    }
    for line in BufReader::new(conn).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Request>(&line) {
            Ok(Request::Search { id, q, limit }) => {
                let t0 = Instant::now();
                latest.store(id, Ordering::SeqCst);
                let r = d.search_response(id, &q, limit, Phase::Instant, t0, 0.0);
                let Response::Search { timing, .. } = &r else { unreachable!() };
                let instant_ms = timing.instant_ms;
                send(&out, &r);
                let _ = tx.send((id, q, limit, t0, instant_ms));
            }
            Ok(req) => send(&out, &d.handle(req)),
            Err(e) => {
                let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v["id"].as_u64()).unwrap_or(0);
                send(&out, &Response::Error { id, error: format!("bad request: {e}") });
            }
        }
    }
}

/// The command line that runs an action (DESIGN.md "Actions").
pub fn command(action: &Action, kind: Kind) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
    match action {
        Action::Launch { desktop_id } => v(&["gtk-launch", desktop_id]),
        Action::Open { path, locator } => match (locator.page, locator.t) {
            (Some(p), _) if path.to_lowercase().ends_with(".pdf") && evince_is_default() => {
                v(&["evince", &format!("--page-index={p}"), path])
            }
            (_, Some(t)) if matches!(kind, Kind::Video | Kind::Audio) => v(&["mpv", &format!("--start={t}"), path]),
            _ => v(&["xdg-open", path]),
        },
        Action::Run { argv } => argv.clone(),
        Action::Copy { text } => v(&["wl-copy", "--", text]),
        Action::Paste { text } => {
            let helper = "/usr/share/omarchy/bin/omarchy-clipboard-paste-text";
            let helper = if std::path::Path::new(helper).exists() { helper } else { "omarchy-clipboard-paste-text" };
            v(&[helper, "--shift-insert", text])
        }
        Action::Url { url } => v(&["xdg-open", url]),
    }
}

fn evince_is_default() -> bool {
    std::process::Command::new("xdg-mime")
        .args(["query", "default", "application/pdf"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("evince"))
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// Run detached, in its own systemd scope when uwsm is there (so it outlives a
/// daemon restart), with no stdio.
#[cfg(not(test))]
fn spawn(argv: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut full: Vec<&str> = if on_path("uwsm-app") { vec!["uwsm-app", "--"] } else { Vec::new() };
    full.extend(argv.iter().map(String::as_str));
    let (prog, args) = full.split_first().context("empty command")?;
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("cannot run {prog}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
pub static RAN: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

#[cfg(test)]
fn spawn(argv: &[String]) -> Result<()> {
    let _ = on_path("uwsm-app");
    RAN.lock().unwrap().push(argv.to_vec());
    Ok(())
}
