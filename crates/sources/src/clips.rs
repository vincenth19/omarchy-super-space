//! Omarchy's clipboard history, and where each clip was copied from.
use crate::{ago, apps, history, now, read_json, sha1, xdg};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ss_core::{Action, Doc, Embed, Kind, Locator, Part, doc_text};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Under the daemon's state folder: {sha1 of clip text: Source}.
pub(crate) const SOURCES: &str = "clip-sources.json";
/// Run by `wl-paste --watch` with each new text clip on stdin: prints its
/// hash, the one clips() computes for the entry Omarchy adds to its history.
/// Copies that Omarchy's capture.sh leaves out of that history print nothing:
/// a password manager's (CLIPBOARD_STATE=sensitive, or offered with
/// x-kde-passwordManagerHint).
const HASH_CLIP: &str = "[ \"$CLIPBOARD_STATE\" = sensitive ] || wl-paste --list-types 2>/dev/null | grep -qx x-kde-passwordManagerHint || sha1sum";
/// Source records kept (Omarchy keeps 100 clips).
const KEEP: usize = 500;

pub(crate) fn history_file() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("omarchy/clipboard-history.json")
}

/// The window a clip was copied from, recorded when it was copied.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Source {
    app: String,
    icon: String,
    class: String,
    title: String,
    at: i64,
}

/// One entry of Omarchy's history: {type: "text", text} or {type: "image", path, mime, capturedAt}.
#[derive(Deserialize)]
struct Clip {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    mime: String,
    #[serde(rename = "capturedAt", default)]
    captured: String,
}

pub fn clips(state: &Path) -> Vec<Doc> {
    clips_checked(state).unwrap_or_default()
}

pub fn clips_checked(state: &Path) -> Option<Vec<Doc>> {
    let sources = load(state);
    // Page URLs come from Chromium's history, which can lag the copy by a few
    // seconds; the History file is watched, so a later read fills them in.
    // Like history rows, they leave out the query and fragment.
    let urls: HashMap<String, String> = match sources.values().any(|s| browser(&s.class)) {
        true => history::visits()?.into_iter().map(|v| (v.title, history::page(&v.url).into())).collect(),
        false => HashMap::new(),
    };
    Some(clip_docs(read_json(&history_file())?, &sources, &urls, now()))
}

fn clip_docs(list: Vec<Clip>, sources: &HashMap<String, Source>, urls: &HashMap<String, String>, now: i64) -> Vec<Doc> {
    list.into_iter()
        .filter_map(|c| match c.kind.as_str() {
            "text" if !c.text.trim().is_empty() => Some(text_doc(c.text, sources, urls, now)),
            "image" if Path::new(&c.path).is_file() => Some(image_doc(c)),
            _ => None,
        })
        .collect()
}

fn text_doc(text: String, sources: &HashMap<String, Source>, urls: &HashMap<String, String>, now: i64) -> Doc {
    let hash = sha1(&text);
    let src = sources.get(&hash);
    let page = src.map_or("", |s| page_title(&s.title));
    let url = src.filter(|s| browser(&s.class)).and_then(|_| urls.get(page)).map_or("", |u| u.as_str());
    let join = |bits: &[&str]| bits.iter().filter(|b| !b.is_empty()).copied().collect::<Vec<_>>().join(" · ");
    let subtitle = match src {
        Some(s) => join(&[&format!("from {}", s.app), if page == s.app { "" } else { page }, &ago(now - s.at)]),
        None => "Clipboard".into(),
    };
    let title = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(120).collect();
    let body: String = text.chars().take(2000).collect();
    // On the Pi a bare link, command or path sat at 0.67-0.70 to nonsense
    // queries. A link copied in a browser embeds as "title: <page> | text:
    // <link>" (closer to the page's topic than "Chromium · <page>" or "link to
    // <site>"), any other link as "link to <site>" when the site is a name;
    // a command as "title: Terminal · ssh | text: ssh command: <cmd>" (finds
    // "that ssh command" at 0.80 while nonsense stays under 0.62; the source
    // app in place of Terminal raised nonsense to 0.65); paths and codes are
    // keywords only. Prose embeds as "title: Chromium · Meetup notes · https://… | text: <clip>".
    let tab = src.filter(|s| browser(&s.class) && !page.is_empty()).map(|_| page);
    let input = match (link_site(&text), tab) {
        (Some(_), Some(tab)) => Some(doc_text(tab, &body)),
        (Some(site), None) => (site.contains('.') && site.contains(|c: char| c.is_ascii_alphabetic())).then(|| doc_text("", &format!("link to {site}"))),
        (None, _) if bare(&text) => command(&text).map(|w| doc_text(&format!("Terminal · {w}"), &format!("{w} command: {body}"))),
        (None, _) => Some(doc_text(&src.map_or(String::new(), |s| join(&[&s.app, page, url])), &body)),
    };
    let stamp = sha1(input.as_deref().unwrap_or(&body));
    let parts = vec![Part { text: body, locator: Locator::default(), embed: input.map_or(Embed::None, Embed::Text) }];
    let icon = src.map_or("", |s| s.icon.as_str());
    Doc {
        key: format!("clip:{hash}"),
        kind: Kind::Clip,
        title,
        subtitle,
        icon: if icon.is_empty() { "edit-paste".into() } else { icon.into() },
        action: Action::Paste { text },
        stamp,
        names: Vec::new(),
        parts,
    }
}

fn image_doc(c: Clip) -> Doc {
    let mime = if c.mime.is_empty() { "image/png".to_string() } else { c.mime };
    Doc {
        key: format!("clip:{}", sha1(&c.path)),
        kind: Kind::Clip,
        title: "Copied image".into(),
        subtitle: if c.captured.is_empty() { "Clipboard".into() } else { format!("Clipboard · {}", c.captured) },
        icon: c.path.clone(),
        action: Action::Run { argv: vec!["omarchy-clipboard-paste-file".into(), mime, c.path.clone()] },
        // Omarchy names clipboard images by the hash of their content.
        stamp: sha1(&c.path),
        names: Vec::new(),
        parts: vec![Part { text: String::new(), locator: Locator::default(), embed: Embed::Image(c.path.into()) }],
    }
}

/// "github.com" for a clip that is only the link "https://www.github.com/a/b".
fn link_site(text: &str) -> Option<String> {
    let t = text.trim();
    let link = (t.starts_with("https://") || t.starts_with("http://")) && !t.contains(char::is_whitespace);
    link.then(|| history::short(t).split(['/', '?', '#', ':']).next().unwrap_or_default().to_string())
}

/// One line that is mostly not words: a command, a path or a code ("ssh pi@10.0.0.42 -p 2222").
fn bare(text: &str) -> bool {
    let toks: Vec<&str> = text.split_whitespace().collect();
    let words = toks.iter().filter(|t| t.trim_end_matches([',', '.', ':', ';', '!', '?']).chars().all(char::is_alphabetic)).count();
    !text.trim().contains('\n') && words * 2 <= toks.len()
}

/// "pacman" for "sudo pacman -Syu --noconfirm": a first word of lowercase
/// letters and dashes (hex codes and ids such as "f47ac10b-…" have digits).
fn command(text: &str) -> Option<&str> {
    text.split_whitespace().find(|w| *w != "sudo").filter(|w| w.starts_with(|c: char| c.is_ascii_lowercase()) && w.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
}

/// Chromium windows and Chromium web apps ("chrome-x.com__-Default").
fn browser(class: &str) -> bool {
    class == "chromium" || class.starts_with("chrome-")
}

/// "Meetup notes" for the window title "Meetup notes - Chromium".
fn page_title(title: &str) -> &str {
    title.strip_suffix(" - Chromium").unwrap_or(title)
}

fn load(state: &Path) -> HashMap<String, Source> {
    read_json(&state.join(SOURCES)).unwrap_or_default()
}

pub fn track_clip_sources(state: &Path) {
    let state = state.to_path_buf();
    std::thread::spawn(move || {
        loop {
            let _ = watch(&state);
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

/// Records the active window for every new text clip, until wl-paste exits
/// (no Wayland session yet, compositor restart).
fn watch(state: &Path) -> std::io::Result<()> {
    let mut child = Command::new("setpriv")
        .args(["--pdeathsig", "TERM", "wl-paste", "--type", "text", "--watch", "sh", "-c", HASH_CLIP])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let Some(out) = child.stdout.take() else { return Ok(()) };
    // wl-paste first reports the clipboard it finds at start: not a new copy.
    let mut startup = history_hashes().into_iter().next();
    for line in std::io::BufReader::new(out).lines() {
        let line = line?;
        let Some(hash) = line.split_whitespace().next() else { continue };
        let first = startup.take();
        // An older clip pasted back (from Omarchy's clipboard menu or Super
        // Space) is copied again; it is not new, so its source stays.
        if first.as_deref() == Some(hash) || history_hashes().iter().skip(1).any(|h| h == hash) {
            continue;
        }
        if let Some(src) = active_window() {
            record(state, hash, src);
        }
    }
    child.wait().map(|_| ())
}

/// sha1 of each history entry's text, newest first ("" for images).
fn history_hashes() -> Vec<String> {
    read_json::<Vec<Clip>>(&history_file()).unwrap_or_default().iter().map(|c| if c.kind == "text" { sha1(&c.text) } else { String::new() }).collect()
}

fn active_window() -> Option<Source> {
    let out = Command::new("hyprctl").args(["activewindow", "-j"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let w: Value = serde_json::from_slice(&out.stdout).ok()?;
    let class = w["class"].as_str().filter(|c| !c.is_empty())?.to_string();
    let (app, icon) = app_for(&class, &apps::entries());
    Some(Source { app, icon, title: w["title"].as_str().unwrap_or("").into(), class, at: crate::now() })
}

/// Name and icon for a window class: the desktop entry with that
/// StartupWMClass or id, or for a Chromium web app the entry that opens its site.
fn app_for(class: &str, entries: &[apps::Entry]) -> (String, String) {
    let site = class.strip_prefix("chrome-").and_then(|r| r.split("__").next()).filter(|s| !s.is_empty());
    let entry = entries
        .iter()
        .find(|e| e.wm_class.eq_ignore_ascii_case(class) || e.id.eq_ignore_ascii_case(class))
        .or_else(|| site.and_then(|s| entries.iter().find(|e| e.exec.contains(&format!("://{s}")))));
    match entry {
        Some(e) => (e.name.clone(), e.icon.clone()),
        None => (site.unwrap_or(class).to_string(), String::new()),
    }
}

/// The first source wins: a clip pasted into another window is not from there.
fn record(state: &Path, hash: &str, src: Source) {
    let mut all = load(state);
    if all.contains_key(hash) {
        return;
    }
    all.insert(hash.into(), src);
    if all.len() > KEEP && let Some(oldest) = all.iter().min_by_key(|(_, s)| s.at).map(|(k, _)| k.clone()) {
        all.remove(&oldest);
    }
    // Written whole and renamed, so clips() never reads half a file.
    let save = || -> std::io::Result<()> {
        std::fs::create_dir_all(state)?;
        let tmp = state.join(format!("{SOURCES}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec(&all)?)?;
        std::fs::rename(tmp, state.join(SOURCES))
    };
    let _ = save();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(app: &str, class: &str, title: &str, at: i64) -> Source {
        Source { app: app.into(), icon: class.into(), class: class.into(), title: title.into(), at }
    }

    #[test]
    fn clips_with_their_sources() {
        let list: Vec<Clip> = serde_json::from_str(
            r##"[
            {"type":"text","text":"https://omarchy.org/plugins"},
            {"type":"text","text":"  \n  bunx wrangler deploy\nsecond line"},
            {"type":"text","text":"#c0644c"},
            {"type":"text","text":"   "},
            {"type":"image","path":"/nonexistent/a.png","mime":"image/png","capturedAt":"Friday 22:57"},
            {"type":"other"},
            {"type":"text","text":"Doors open at 6pm, talks at 6:30"},
            {"type":"text","text":"https://github.com/vincenth19/omarchy-super-space"},
            {"type":"text","text":"http://10.0.0.42:8080/admin"},
            {"type":"text","text":"ssh pi@10.0.0.42 -p 2222"},
            {"type":"text","text":"~/Documents/lease.pdf"},
            {"type":"text","text":"sudo pacman -Syu --noconfirm"},
            {"type":"text","text":"f47ac10b-58cc-4372-a567-0e02b2c3d479"}
        ]"##,
        )
        .unwrap();
        let sources: HashMap<String, Source> = [
            (sha1("https://omarchy.org/plugins"), src("Chromium", "chromium", "Meetup notes - Chromium", 1000 - 7200)),
            (sha1("  \n  bunx wrangler deploy\nsecond line"), src("Ghostty", "com.mitchellh.ghostty", "Ghostty", 1000 - 90)),
            (sha1("Doors open at 6pm, talks at 6:30"), src("Chromium", "chromium", "Meetup notes - Chromium", 1000)),
        ]
        .into();
        let urls: HashMap<String, String> = [("Meetup notes".to_string(), "https://docs.example/meetup".to_string())].into();
        let docs = clip_docs(list, &sources, &urls, 1000);
        let rows: Vec<(&str, &str, &str)> = docs.iter().map(|d| (d.title.as_str(), d.subtitle.as_str(), d.icon.as_str())).collect();
        assert_eq!(
            rows[..3],
            [
                ("https://omarchy.org/plugins", "from Chromium · Meetup notes · 2 h ago", "chromium"),
                ("bunx wrangler deploy", "from Ghostty · 1 min ago", "com.mitchellh.ghostty"),
                ("#c0644c", "Clipboard", "edit-paste"),
            ]
        );
        assert_eq!(docs[0].key, "clip:23fa0cd44b128e1508c384b57a8b18e7264f1b54");
        assert_eq!(docs[0].action, Action::Paste { text: "https://omarchy.org/plugins".into() });
        // Links by their page or site, commands by their program; paths, codes and links to bare addresses keywords only.
        let embeds: Vec<Option<&str>> = docs.iter().map(|d| if let Embed::Text(t) = &d.parts[0].embed { Some(t.as_str()) } else { None }).collect();
        assert_eq!(
            embeds,
            [
                Some("title: Meetup notes | text: https://omarchy.org/plugins"),
                Some("title: Ghostty · Ghostty | text:   \n  bunx wrangler deploy\nsecond line"),
                None,
                Some("title: Chromium · Meetup notes · https://docs.example/meetup | text: Doors open at 6pm, talks at 6:30"),
                Some("title: none | text: link to github.com"),
                None,
                Some("title: Terminal · ssh | text: ssh command: ssh pi@10.0.0.42 -p 2222"),
                None,
                Some("title: Terminal · pacman | text: pacman command: sudo pacman -Syu --noconfirm"),
                None,
            ]
        );
        assert_eq!(docs[7].parts[0].text, "~/Documents/lease.pdf");
    }

    #[test]
    fn password_manager_copies_are_not_hashed() {
        let dir = std::env::temp_dir().join(format!("ss-sources-wlpaste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("wl-paste");
        std::fs::write(&fake, "#!/bin/sh\nprintf '%s\\n' \"$TYPES\"\n").unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
        let hash = |state: &str, types: &str| {
            let mut c = Command::new("sh").args(["-c", HASH_CLIP]).env("PATH", &path).env("CLIPBOARD_STATE", state).env("TYPES", types).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
            std::io::Write::write_all(&mut c.stdin.take().unwrap(), b"hunter2").unwrap();
            String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap()
        };
        assert_eq!(hash("data", "text/plain\nUTF8_STRING").split_whitespace().next(), Some(sha1("hunter2").as_str()));
        assert_eq!(hash("sensitive", "text/plain"), "");
        assert_eq!(hash("data", "text/plain\nx-kde-passwordManagerHint"), "");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn image_clips_paste_the_file() {
        let path = std::env::temp_dir().join(format!("ss-sources-clip-{}.png", std::process::id()));
        std::fs::write(&path, b"png").unwrap();
        let p = path.to_string_lossy().to_string();
        let list: Vec<Clip> = serde_json::from_value(serde_json::json!([{"type":"image","path":p,"mime":"image/png","capturedAt":"Friday 22:57"}])).unwrap();
        let docs = clip_docs(list, &HashMap::new(), &HashMap::new(), 0);
        assert_eq!(docs[0].subtitle, "Clipboard · Friday 22:57");
        assert_eq!(docs[0].action, Action::Run { argv: vec!["omarchy-clipboard-paste-file".into(), "image/png".into(), p.clone()] });
        assert!(matches!(&docs[0].parts[0].embed, Embed::Image(i) if *i == path));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn window_classes_to_apps() {
        let entry = |id: &str, name: &str, class: &str, exec: &str| apps::Entry { id: id.into(), name: name.into(), generic: String::new(), comment: String::new(), keywords: Vec::new(), icon: id.to_lowercase(), wm_class: class.into(), exec: exec.into(), categories: String::new() };
        let entries = [entry("chromium", "Chromium", "chromium", "/usr/bin/chromium %U"), entry("X", "X", "", "omarchy-launch-webapp https://x.com/"), entry("libreoffice-writer", "LibreOffice Writer", "libreoffice-writer", "")];
        assert_eq!(app_for("chromium", &entries), ("Chromium".into(), "chromium".into()));
        assert_eq!(app_for("chrome-x.com__-Default", &entries), ("X".into(), "x".into()));
        assert_eq!(app_for("chrome-app.hey.com__-Default", &entries), ("app.hey.com".into(), String::new()));
        assert_eq!(app_for("Alacritty", &entries), ("Alacritty".into(), String::new()));
    }

    #[test]
    fn first_source_wins_and_old_records_go() {
        let state = std::env::temp_dir().join(format!("ss-sources-state-{}", std::process::id()));
        record(&state, "a", src("Chromium", "chromium", "One", 5));
        record(&state, "a", src("Ghostty", "ghostty", "Two", 6));
        assert_eq!(load(&state)["a"].app, "Chromium");
        for i in 0..KEEP {
            record(&state, &format!("h{i}"), src("App", "app", "", 10 + i as i64));
        }
        let all = load(&state);
        assert_eq!(all.len(), KEEP);
        assert!(!all.contains_key("a"));
        std::fs::remove_dir_all(state).unwrap();
    }
}
