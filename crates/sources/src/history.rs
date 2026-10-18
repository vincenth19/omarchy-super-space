//! Chromium history and bookmarks. History is read from a copy: Chromium
//! keeps the live file open and locked.
use crate::{ago, now, read_json, text_part, xdg};
use serde::Deserialize;
use serde_json::Value;
use ss_core::{Action, Doc, Kind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Most recent pages read; each one becomes a row that is embedded once.
const LIMIT: usize = 3000;

fn profile() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("chromium/Default")
}

pub(crate) fn files() -> Vec<PathBuf> {
    vec![profile().join("History"), profile().join("Bookmarks")]
}

#[derive(Deserialize)]
pub(crate) struct Visit {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Unix seconds.
    pub t: i64,
}

pub fn history() -> Vec<Doc> {
    history_checked().unwrap_or_default()
}

pub fn history_checked() -> Option<Vec<Doc>> {
    Some(history_docs(&bookmarks(&read_json(&profile().join("Bookmarks"))?), &visits()?, now()))
}

pub(crate) fn visits() -> Option<Vec<Visit>> {
    visits_in(&profile().join("History"))
}

/// The latest visit to each page title, newest first, read from a copy of `db`
/// in $XDG_RUNTIME_DIR that is deleted straight after. None when the copy or
/// sqlite3 fails (a torn copy stops with "malformed").
fn visits_in(db: &Path) -> Option<Vec<Visit>> {
    static N: AtomicUsize = AtomicUsize::new(0);
    if !db.exists() {
        return Some(Vec::new());
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let copy = runtime.join(format!("superspace-history-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    // Chrome time is microseconds since 1601.
    let sql = format!("select url, title, last_visit_time / 1000000 - 11644473600 as t from urls where hidden = 0 and url like 'http%' order by last_visit_time desc limit {LIMIT}");
    let out = std::fs::copy(db, &copy).ok().and_then(|_| std::process::Command::new("sqlite3").args(["-readonly", "-json"]).arg(&copy).arg(sql).output().ok());
    let _ = std::fs::remove_file(&copy);
    let out = out.filter(|o| o.status.success())?;
    // sqlite3 prints nothing at all when no row matches.
    let all: Vec<Visit> = if out.stdout.is_empty() { Vec::new() } else { serde_json::from_slice(&out.stdout).ok()? };
    // Pages that share a title ("Home / X") cannot be told apart in a list; keep the latest.
    let mut seen = HashSet::new();
    Some(all.into_iter().filter(|v| seen.insert(if v.title.is_empty() { v.url.clone() } else { v.title.clone() })).collect())
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
/// the latest visit.
fn history_docs(marks: &[(String, String)], visits: &[Visit], now: i64) -> Vec<Doc> {
    let marks = marks.iter().map(|(url, title)| (url, title, "Bookmark".to_string()));
    let visits = visits.iter().map(|v| (&v.url, &v.title, ago(now - v.t)));
    let mut seen = HashSet::new();
    marks
        .chain(visits)
        .filter(|(url, ..)| url.starts_with("http") && seen.insert(page(url)))
        .map(|(url, title, when)| {
            let page = page(url);
            let title = if title.is_empty() { short(page) } else { title.clone() };
            let (stamp, parts) = text_part(&title, page.into());
            Doc { key: format!("history:{page}"), kind: Kind::History, title, subtitle: format!("{} · {when}", short(page)), icon: "chromium".into(), action: Action::Url { url: url.clone() }, stamp, names: Vec::new(), parts }
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

    #[test]
    fn bookmarks_first_then_visits() {
        let json: Value = serde_json::from_str(
            r#"{"roots":{"bookmark_bar":{"type":"folder","children":[
                {"type":"url","name":"Omarchy manual","url":"https://omarchy.org/manual/"},
                {"type":"folder","name":"Dev","children":[{"type":"url","name":"","url":"https://github.com/basecamp/omarchy"}]}]},
              "other":{"type":"folder","children":[{"type":"url","name":"settings","url":"chrome://settings"}]},
              "sync_transaction_version":"1"}}"#,
        )
        .unwrap();
        let marks = bookmarks(&json);
        assert_eq!(marks.len(), 3);
        let visits = vec![Visit { url: "https://omarchy.org/manual/".into(), title: "Manual".into(), t: 1000 }, Visit { url: "https://www.example.com/a/".into(), title: "Example".into(), t: 1000 - 7200 }];
        let docs = history_docs(&marks, &visits, 1000);
        let rows: Vec<(&str, &str, &str)> = docs.iter().map(|d| (d.key.as_str(), d.title.as_str(), d.subtitle.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("history:https://omarchy.org/manual/", "Omarchy manual", "omarchy.org/manual · Bookmark"),
                ("history:https://github.com/basecamp/omarchy", "github.com/basecamp/omarchy", "github.com/basecamp/omarchy · Bookmark"),
                ("history:https://www.example.com/a/", "Example", "example.com/a · 2 h ago"),
            ]
        );
        assert_eq!(docs[2].action, Action::Url { url: "https://www.example.com/a/".into() });
    }

    #[test]
    fn queries_and_fragments_only_in_the_action() {
        let marks = [("https://app.example/login#top".to_string(), "".to_string())];
        let visits = vec![
            Visit { url: "https://app.example/login?token=s3cret".into(), title: "Sign in".into(), t: 1000 },
            Visit { url: "https://claude.com/cai/oauth/authorize?code_challenge=abc&state=TLdT".into(), title: "Authorize".into(), t: 1000 },
        ];
        let docs = history_docs(&marks, &visits, 1000);
        assert_eq!(docs.len(), 2);
        let d = &docs[1];
        assert_eq!((d.key.as_str(), d.subtitle.as_str(), d.parts[0].text.as_str()), ("history:https://claude.com/cai/oauth/authorize", "claude.com/cai/oauth/authorize · just now", "https://claude.com/cai/oauth/authorize"));
        assert!(matches!(&d.parts[0].embed, ss_core::Embed::Text(t) if t == "title: Authorize | text: https://claude.com/cai/oauth/authorize"));
        assert_eq!(d.action, Action::Url { url: "https://claude.com/cai/oauth/authorize?code_challenge=abc&state=TLdT".into() });
        // The login page is one row, the bookmark's.
        assert_eq!((docs[0].key.as_str(), docs[0].title.as_str()), ("history:https://app.example/login", "app.example/login"));
        assert_eq!(docs[0].action, Action::Url { url: "https://app.example/login#top".into() });
    }

    #[test]
    fn reads_a_copy_of_a_chromium_history_db() {
        let dir = std::env::temp_dir().join(format!("ss-sources-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("History");
        // 13436788256133210 is 18 Oct 2026, 09:10:56 UTC in Chrome time.
        let sql = "create table urls(id integer primary key, url text, title text, visit_count int, typed_count int, last_visit_time int, hidden int);
            insert into urls(url, title, last_visit_time, hidden) values
              ('https://x.com/home', 'Home / X', 13436788205420958, 0),
              ('https://x.com/a/status/1', 'Home / X', 13436788256133210, 0),
              ('https://omarchy.org/', 'Omarchy', 13436700000000000, 0),
              ('https://hidden.example/', 'Hidden', 13436788256133210, 1),
              ('file:///etc/passwd', 'passwd', 13436788256133210, 0);";
        assert!(std::process::Command::new("sqlite3").arg(&db).arg(sql).status().unwrap().success());
        assert_eq!(visits_in(&dir.join("none")).map(|v| v.len()), Some(0));
        let v = visits_in(&db).unwrap();
        let got: Vec<(&str, i64)> = v.iter().map(|v| (v.url.as_str(), v.t)).collect();
        assert_eq!(got, [("https://x.com/a/status/1", 1792314656), ("https://omarchy.org/", 1792226400)]);
        assert!(std::process::Command::new("sqlite3").arg(&db).arg("delete from urls").status().unwrap().success());
        assert_eq!(visits_in(&db).map(|v| v.len()), Some(0));
        // A torn copy is a failed read, not an empty history.
        std::fs::write(&db, b"SQLite format 3\0torn").unwrap();
        assert!(visits_in(&db).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
