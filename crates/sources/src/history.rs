//! Browser history and bookmarks: every profile of Chromium, Chrome, Brave and
//! Edge (`History`, `Bookmarks`) and of Firefox and Zen (`places.sqlite`).
//! Databases are read from copies: browsers keep the live files open and locked.
use crate::{ago, apps, clips, files_under, home, now, read_json, text_part, xdg};
use serde::Deserialize;
use serde_json::Value;
use ss_core::{Action, Doc, Kind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Most recent pages read; each one becomes a row that is embedded once.
const LIMIT: usize = 3000;

/// Name, desktop entry (id or window class, and the icon when there is no entry), the
/// folder of its profiles under $HOME (".config/…" under $XDG_CONFIG_HOME), and whether
/// it keeps history the Firefox way. Firefox 147 and later make new profiles under ~/.config.
const BROWSERS: [(&str, &str, &str, bool); 8] = [
    ("Chromium", "chromium", ".config/chromium", false),
    ("Chrome", "google-chrome", ".config/google-chrome", false),
    ("Brave", "brave-browser", ".config/BraveSoftware/Brave-Browser", false),
    ("Edge", "microsoft-edge", ".config/microsoft-edge", false),
    ("Firefox", "firefox", ".mozilla/firefox", true),
    ("Firefox", "firefox", ".config/mozilla/firefox", true),
    ("Zen", "zen", ".zen", true),
    ("Zen", "zen", ".config/zen", true),
];

/// Every profile's history database, with its browser's place in BROWSERS.
fn profiles(home: &Path, config: &Path) -> Vec<(usize, PathBuf)> {
    let dbs = BROWSERS.iter().enumerate().map(|(i, &(_, _, dir, firefox))| {
        let root = dir.strip_prefix(".config/").map_or_else(|| home.join(dir), |d| config.join(d));
        let name = if firefox { "places.sqlite" } else { "History" };
        files_under(&root, 2, &|n| n == name).into_iter().map(move |db| (i, db))
    });
    dbs.flatten().collect()
}

fn installed() -> Vec<(usize, PathBuf)> {
    profiles(&home(), &xdg("XDG_CONFIG_HOME", ".config"))
}

/// The databases, with Chromium's bookmarks and Firefox's write-ahead log, which holds its latest visits.
pub(crate) fn files() -> Vec<PathBuf> {
    let both = |(i, db): (usize, PathBuf)| [if BROWSERS[i].3 { with(&db, "-wal") } else { db.with_file_name("Bookmarks") }, db];
    installed().into_iter().flat_map(both).collect()
}

#[derive(Deserialize)]
pub(crate) struct Visit {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Unix seconds.
    pub t: i64,
    /// Its browser's place in BROWSERS.
    #[serde(skip)]
    from: usize,
}

pub fn history() -> Vec<Doc> {
    history_checked().unwrap_or_default()
}

pub fn history_checked() -> Option<Vec<Doc>> {
    let profiles = installed();
    let mut marks = Vec::new();
    for (i, db) in profiles.iter().filter(|p| !BROWSERS[p.0].3) {
        marks.extend(bookmarks(&read_json(&db.with_file_name("Bookmarks"))?).into_iter().map(|(url, title)| (url, title, *i)));
    }
    Some(history_docs(&marks, &visits_of(&profiles)?, &icons(), now()))
}

pub(crate) fn visits() -> Option<Vec<Visit>> {
    visits_of(&installed())
}

/// The latest visit to each page title over every profile, newest first. None when
/// one could not be read: its rows must stay.
fn visits_of(profiles: &[(usize, PathBuf)]) -> Option<Vec<Visit>> {
    let mut all = Vec::new();
    for (i, db) in profiles {
        all.extend(visits_in(db, BROWSERS[*i].3)?.into_iter().map(|v| Visit { from: *i, ..v }));
    }
    all.sort_by_key(|v| -v.t);
    // Pages that share a title ("Home / X") cannot be told apart in a list; keep the latest.
    let mut seen = HashSet::new();
    all.retain(|v| seen.insert(if v.title.is_empty() { v.url.clone() } else { v.title.clone() }));
    all.truncate(LIMIT);
    Some(all)
}

/// `p` with `ext` added: "places.sqlite-wal" for "places.sqlite".
fn with(p: &Path, ext: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(ext);
    s.into()
}

/// The most recent visits in `db`, read from a copy in $XDG_RUNTIME_DIR (with
/// its write-ahead log) that is deleted straight after. None when the copy or
/// sqlite3 fails (a torn copy stops with "malformed").
fn visits_in(db: &Path, firefox: bool) -> Option<Vec<Visit>> {
    static N: AtomicUsize = AtomicUsize::new(0);
    if !db.exists() {
        return Some(Vec::new());
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let copy = runtime.join(format!("superspace-history-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    // Chrome time is microseconds since 1601, Firefox time microseconds since 1970.
    let sql = if firefox {
        format!("select url, coalesce(title, '') as title, last_visit_date / 1000000 as t from moz_places where hidden = 0 and url like 'http%' and last_visit_date > 0 order by last_visit_date desc limit {LIMIT}")
    } else {
        format!("select url, title, last_visit_time / 1000000 - 11644473600 as t from urls where hidden = 0 and url like 'http%' order by last_visit_time desc limit {LIMIT}")
    };
    let wal = with(db, "-wal");
    let copied = std::fs::copy(db, &copy).is_ok() && (!wal.exists() || std::fs::copy(&wal, with(&copy, "-wal")).is_ok());
    let out = copied.then(|| std::process::Command::new("sqlite3").args(["-readonly", "-json"]).arg(&copy).arg(sql).output().ok()).flatten();
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(with(&copy, ext));
    }
    let out = out.filter(|o| o.status.success())?;
    // sqlite3 prints nothing at all when no row matches.
    if out.stdout.is_empty() { Some(Vec::new()) } else { serde_json::from_slice(&out.stdout).ok() }
}

/// Each browser's icon: its desktop entry's, else the entry's id.
fn icons() -> Vec<String> {
    let entries = apps::entries();
    BROWSERS.iter().map(|b| Some(clips::app_for(b.1, &entries).1).filter(|i| !i.is_empty()).unwrap_or_else(|| b.1.into())).collect()
}

/// (url, title) of every bookmark, in Chromium's Bookmarks JSON.
fn bookmarks(json: &Value) -> Vec<(String, String)> {
    fn walk(n: &Value, out: &mut Vec<(String, String)>) {
        if let (Some("url"), Some(url)) = (n["type"].as_str(), n["url"].as_str()) {
            out.push((url.into(), n["name"].as_str().unwrap_or("").into()));
        }
        n["children"].as_array().into_iter().flatten().for_each(|c| walk(c, out));
    }
    let mut out = Vec::new();
    json["roots"].as_object().into_iter().flat_map(|o| o.values()).for_each(|r| walk(r, &mut out));
    out
}

/// Rows are indexed, keyed and shown by the page without its query and
/// fragment, where login links and reset tokens sit; only the action opens
/// the full URL. Pages that differ only there are one row: the bookmark, or
/// the latest visit in any browser. Marks are (url, title, browser).
fn history_docs(marks: &[(String, String, usize)], visits: &[Visit], icons: &[String], now: i64) -> Vec<Doc> {
    let marks = marks.iter().map(|(url, title, from)| (url, title, *from, "Bookmark".to_string()));
    let visits = visits.iter().map(|v| (&v.url, &v.title, v.from, ago(now - v.t)));
    let mut seen = HashSet::new();
    marks
        .chain(visits)
        .filter(|(url, ..)| url.starts_with("http") && seen.insert(page(url)))
        .map(|(url, title, from, when)| {
            let page = page(url);
            let title = if title.is_empty() { short(page) } else { title.clone() };
            let (stamp, parts) = text_part(&title, page.into());
            let subtitle = format!("{} · {} · {when}", BROWSERS[from].0, short(page));
            Doc { key: format!("history:{page}"), kind: Kind::History, title, subtitle, icon: icons[from].clone(), action: Action::Url { url: url.clone() }, stamp, names: Vec::new(), parts }
        })
        .collect()
}

/// "https://a.example/login" for "https://a.example/login?token=x#y".
pub(crate) fn page(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// "omarchy.org/plugins" for "https://www.omarchy.org/plugins/".
pub(crate) fn short(url: &str) -> String {
    let s = url.split_once("://").map_or(url, |(_, rest)| rest);
    s.strip_prefix("www.").unwrap_or(s).trim_end_matches('/').chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(url: &str, title: &str, t: i64, from: usize) -> Visit {
        Visit { url: url.into(), title: title.into(), t, from }
    }

    fn icons() -> Vec<String> {
        BROWSERS.iter().map(|b| b.1.to_string()).collect()
    }

    #[test]
    fn bookmarks_first_then_visits_with_their_browsers() {
        let json: Value = serde_json::from_str(
            r#"{"roots":{"bookmark_bar":{"type":"folder","children":[
                {"type":"url","name":"Omarchy manual","url":"https://omarchy.org/manual/"},
                {"type":"folder","name":"Dev","children":[{"type":"url","name":"","url":"https://github.com/basecamp/omarchy"}]}]},
              "other":{"type":"folder","children":[{"type":"url","name":"settings","url":"chrome://settings"}]},
              "sync_transaction_version":"1"}}"#,
        )
        .unwrap();
        let marks: Vec<(String, String, usize)> = bookmarks(&json).into_iter().map(|(u, t)| (u, t, 0)).collect();
        assert_eq!(marks.len(), 3);
        let visits = vec![visit("https://omarchy.org/manual/", "Manual", 1000, 4), visit("https://www.example.com/a/", "Example", 1000 - 7200, 4), visit("https://news.example/", "News", 1000 - 60, 6)];
        let docs = history_docs(&marks, &visits, &icons(), 1000);
        let rows: Vec<(&str, &str, &str, &str)> = docs.iter().map(|d| (d.key.as_str(), d.title.as_str(), d.subtitle.as_str(), d.icon.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("history:https://omarchy.org/manual/", "Omarchy manual", "Chromium · omarchy.org/manual · Bookmark", "chromium"),
                ("history:https://github.com/basecamp/omarchy", "github.com/basecamp/omarchy", "Chromium · github.com/basecamp/omarchy · Bookmark", "chromium"),
                ("history:https://www.example.com/a/", "Example", "Firefox · example.com/a · 2 h ago", "firefox"),
                ("history:https://news.example/", "News", "Zen · news.example · 1 min ago", "zen"),
            ]
        );
        assert_eq!(docs[2].action, Action::Url { url: "https://www.example.com/a/".into() });
    }

    #[test]
    fn queries_and_fragments_only_in_the_action() {
        let marks = [("https://app.example/login#top".to_string(), "".to_string(), 0)];
        let visits = vec![visit("https://app.example/login?token=s3cret", "Sign in", 1000, 0), visit("https://claude.com/cai/oauth/authorize?code_challenge=abc&state=TLdT", "Authorize", 1000, 2)];
        let docs = history_docs(&marks, &visits, &icons(), 1000);
        assert_eq!(docs.len(), 2);
        let d = &docs[1];
        assert_eq!((d.key.as_str(), d.subtitle.as_str(), d.parts[0].text.as_str()), ("history:https://claude.com/cai/oauth/authorize", "Brave · claude.com/cai/oauth/authorize · just now", "https://claude.com/cai/oauth/authorize"));
        assert!(matches!(&d.parts[0].embed, ss_core::Embed::Text(t) if t == "title: Authorize | text: https://claude.com/cai/oauth/authorize"));
        assert_eq!(d.action, Action::Url { url: "https://claude.com/cai/oauth/authorize?code_challenge=abc&state=TLdT".into() });
        // The login page is one row, the bookmark's.
        assert_eq!((docs[0].key.as_str(), docs[0].title.as_str()), ("history:https://app.example/login", "app.example/login"));
        assert_eq!(docs[0].action, Action::Url { url: "https://app.example/login#top".into() });
    }

    fn sqlite(db: &Path, sql: &str) {
        assert!(std::process::Command::new("sqlite3").arg(db).arg(sql).status().unwrap().success(), "{sql}");
    }

    /// Chromium-style History: 13436788256133210 is 18 Oct 2026, 09:10:56 UTC in Chrome time.
    fn chromium_db(db: &Path, rows: &str) {
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        sqlite(db, &format!("create table urls(id integer primary key, url text, title text, visit_count int, typed_count int, last_visit_time int, hidden int);
            insert into urls(url, title, last_visit_time, hidden) values {rows};"));
    }

    const FIREFOX: &str = "create table moz_places(id integer primary key, url text, title text, rev_host text, visit_count int, hidden int not null default 0, last_visit_date int);
        create table moz_historyvisits(id integer primary key, from_visit int, place_id int, visit_date int, visit_type int);";

    #[test]
    fn reads_copies_of_every_profile_of_every_browser() {
        let dir = std::env::temp_dir().join(format!("ss-sources-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (home, config) = (dir.join("home"), dir.join("config"));
        chromium_db(
            &config.join("chromium/Default/History"),
            "('https://x.com/home', 'Home / X', 13436788205420958, 0),
             ('https://x.com/a/status/1', 'Home / X', 13436788256133210, 0),
             ('https://omarchy.org/', 'Omarchy', 13436700000000000, 0),
             ('https://hidden.example/', 'Hidden', 13436788256133210, 1),
             ('file:///etc/passwd', 'passwd', 13436788256133210, 0)",
        );
        chromium_db(&config.join("BraveSoftware/Brave-Browser/Profile 1/History"), "('https://search.brave.com/', 'Brave Search', 13436788000000000, 0)");
        std::fs::create_dir_all(config.join("google-chrome")).unwrap(); // installed, never opened
        // Zen: an old profile, written and closed.
        let zen = home.join(".zen/abcd.Default (release)/places.sqlite");
        std::fs::create_dir_all(zen.parent().unwrap()).unwrap();
        sqlite(&zen, &format!("{FIREFOX} insert into moz_places(url, title, hidden, last_visit_date) values
            ('https://zen-browser.app/', NULL, 0, 1792310000000000), ('https://only.bookmarked/', 'Never visited', 0, NULL);"));
        // Firefox, running: its latest visits are still in the write-ahead log.
        let ff = home.join(".mozilla/firefox/x1y2.default-release/places.sqlite");
        std::fs::create_dir_all(ff.parent().unwrap()).unwrap();
        let mut running = std::process::Command::new("sqlite3").arg(&ff).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).spawn().unwrap();
        let mut stdin = running.stdin.take().unwrap();
        let ready = dir.join("ready");
        // .once (at the start of a line) writes and closes its file: a pipe would hold "ready" until sqlite3 exits.
        std::io::Write::write_all(&mut stdin, format!("pragma journal_mode=wal; {FIREFOX}
            insert into moz_places(url, title, hidden, last_visit_date) values ('https://www.mozilla.org/en-US/firefox/?utm=x', 'Firefox', 0, 1792314000000000);
            insert into moz_historyvisits(place_id, visit_date) values (1, 1792314000000000);
.once '{}'
select 'ready';\n", ready.display()).as_bytes()).unwrap();
        let t = std::time::Instant::now();
        while !std::fs::read_to_string(&ready).is_ok_and(|s| s.contains("ready")) {
            assert!(t.elapsed().as_secs() < 10, "sqlite3 never got ready");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(with(&ff, "-wal").exists(), "the visit is only in the log");

        let profiles = profiles(&home, &config);
        let found: Vec<(&str, String)> = profiles.iter().map(|(i, db)| (BROWSERS[*i].0, db.strip_prefix(&dir).unwrap().display().to_string())).collect();
        assert_eq!(
            found,
            [
                ("Chromium", "config/chromium/Default/History".into()),
                ("Brave", "config/BraveSoftware/Brave-Browser/Profile 1/History".into()),
                ("Firefox", "home/.mozilla/firefox/x1y2.default-release/places.sqlite".into()),
                ("Zen", "home/.zen/abcd.Default (release)/places.sqlite".into()),
            ]
        );
        let v = visits_of(&profiles).unwrap();
        let got: Vec<(&str, &str, i64, &str)> = v.iter().map(|v| (v.url.as_str(), v.title.as_str(), v.t, BROWSERS[v.from].0)).collect();
        assert_eq!(
            got,
            [
                ("https://x.com/a/status/1", "Home / X", 1792314656, "Chromium"),
                ("https://search.brave.com/", "Brave Search", 1792314400, "Brave"),
                ("https://www.mozilla.org/en-US/firefox/?utm=x", "Firefox", 1792314000, "Firefox"),
                ("https://zen-browser.app/", "", 1792310000, "Zen"),
                ("https://omarchy.org/", "Omarchy", 1792226400, "Chromium"),
            ]
        );
        drop(stdin);
        running.wait().unwrap();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let left = std::fs::read_dir(runtime).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(&format!("superspace-history-{}-", std::process::id())));
        assert!(!left, "copies and their logs are deleted");

        let db = config.join("chromium/Default/History");
        sqlite(&db, "delete from urls");
        assert_eq!(visits_in(&db, false).map(|v| v.len()), Some(0));
        assert_eq!(visits_in(&dir.join("none"), false).map(|v| v.len()), Some(0));
        // A torn copy is a failed read, not an empty history, and the other browsers' rows wait too.
        std::fs::write(&db, b"SQLite format 3\0torn").unwrap();
        assert!(visits_in(&db, false).is_none() && visits_of(&profiles).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
