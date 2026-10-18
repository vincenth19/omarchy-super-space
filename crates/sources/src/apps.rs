//! Desktop entries, filtered the way Omarchy's launcher filters them.
use crate::{files_under, omarchy, text_part, xdg};
use ss_core::{Action, Doc, Kind};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) struct Entry {
    /// Desktop id without ".desktop", e.g. "org.gnome.Nautilus".
    pub id: String,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub keywords: Vec<String>,
    pub icon: String,
    pub wm_class: String,
    pub exec: String,
}

pub fn apps() -> Vec<Doc> {
    entries().into_iter().map(doc).collect()
}

fn doc(e: Entry) -> Doc {
    let body = [e.generic.clone(), e.comment.clone(), e.keywords.join(", ")]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(". ");
    let (stamp, parts) = text_part(&e.name, body);
    let mut names = e.keywords;
    if !e.generic.is_empty() {
        names.insert(0, e.generic.clone());
    }
    Doc {
        key: format!("app:{}", e.id),
        kind: Kind::App,
        subtitle: if e.comment.is_empty() { e.generic } else { e.comment },
        icon: if e.icon.is_empty() { "application-x-executable".into() } else { e.icon },
        action: Action::Launch { desktop_id: format!("{}.desktop", e.id) },
        title: e.name,
        stamp,
        names,
        parts,
    }
}

/// Application folders, most important first: the user's copy of an entry
/// overrides the system one.
pub(crate) fn dirs() -> Vec<PathBuf> {
    let data = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty());
    let mut dirs = vec![xdg("XDG_DATA_HOME", ".local/share").join("applications")];
    dirs.extend(data.as_deref().unwrap_or("/usr/local/share:/usr/share").split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("applications")));
    dirs
}

/// Entries Omarchy's launcher never lists.
pub(crate) fn hides_file() -> PathBuf {
    omarchy().join("default/omarchy/launcher.hides")
}

pub(crate) fn entries() -> Vec<Entry> {
    let hides = std::fs::read_to_string(hides_file()).unwrap_or_default();
    scan(&dirs(), &hides, &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
}

fn scan(dirs: &[PathBuf], hides: &str, desktops: &str) -> Vec<Entry> {
    let hides: HashSet<&str> = hides.lines().map(|l| l.trim().trim_end_matches(".desktop")).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for dir in dirs {
        for file in files_under(dir, 4, &|n| n.ends_with(".desktop")) {
            let rel = file.strip_prefix(dir).unwrap_or(&file).to_string_lossy();
            let id = rel.trim_end_matches(".desktop").replace('/', "-");
            // The first file for an id decides, so a hidden user copy hides the system entry too.
            if !seen.insert(id.clone()) || hides.contains(id.as_str()) {
                continue;
            }
            if let Some(e) = parse(&id, &std::fs::read_to_string(&file).unwrap_or_default(), desktops) {
                out.push(e);
            }
        }
    }
    out
}

/// The visible application in one .desktop file, or None when it is hidden,
/// not for this desktop (OnlyShowIn/NotShowIn) or not an application.
fn parse(id: &str, text: &str, desktops: &str) -> Option<Entry> {
    let mut keys = HashMap::new();
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if let Some((k, v)) = line.split_once('=').filter(|_| in_entry && !line.starts_with('#')) {
            keys.entry(k.trim()).or_insert_with(|| unescape(v.trim()));
        }
    }
    let get = |k: &str| keys.get(k).cloned().unwrap_or_default();
    let ours = |k: &str| get(k).split(';').any(|d| !d.is_empty() && desktops.split(':').any(|x| x == d));
    let hidden = get("NoDisplay") == "true" || get("Hidden") == "true";
    let elsewhere = (keys.contains_key("OnlyShowIn") && !ours("OnlyShowIn")) || ours("NotShowIn");
    if hidden || elsewhere || !matches!(get("Type").as_str(), "" | "Application") || get("Name").is_empty() {
        return None;
    }
    Some(Entry {
        id: id.into(),
        name: get("Name"),
        generic: get("GenericName"),
        comment: get("Comment"),
        keywords: get("Keywords").split(';').map(str::trim).filter(|k| !k.is_empty()).map(String::from).collect(),
        icon: get("Icon"),
        wm_class: get("StartupWMClass"),
        exec: get("Exec"),
    })
}

/// Desktop Entry escapes: \s \n \t \r \\.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, text: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn user_copies_override_and_hidden_entries_go() {
        let root = std::env::temp_dir().join(format!("ss-sources-apps-{}", std::process::id()));
        let (user, system) = (root.join("user"), root.join("system"));
        write(&system, "firefox.desktop", "[Desktop Entry]\nType=Application\nName=Firefox\nGenericName=Web Browser\nComment=Browse the Web\nKeywords=Internet;WWW;\nIcon=firefox\n[Desktop Action new]\nName=New Window\n");
        write(&user, "firefox.desktop", "[Desktop Entry]\nType=Application\nName=My Firefox\nIcon=/home/me/ff.png\n");
        write(&system, "nautilus.desktop", "[Desktop Entry]\nName=Files\n");
        write(&user, "nautilus.desktop", "[Desktop Entry]\nName=Files\nHidden=true\n");
        write(&system, "gnome-only.desktop", "[Desktop Entry]\nName=Gnome thing\nOnlyShowIn=GNOME;\n");
        write(&system, "not-here.desktop", "[Desktop Entry]\nName=Not on Hyprland\nNotShowIn=Hyprland;KDE;\n");
        write(&system, "nodisplay.desktop", "[Desktop Entry]\nName=Helper\nNoDisplay=true\n");
        write(&system, "link.desktop", "[Desktop Entry]\nType=Link\nName=A link\nURL=https://x\n");
        write(&system, "btop.desktop", "[Desktop Entry]\nName=btop++\n");
        write(&system, "wine/Programs/notepad.desktop", "[Desktop Entry]\nName=Notepad\nComment=Edit\\stext\\nfiles\n");

        let apps: Vec<Doc> = scan(&[user, system], "btop\n", "Hyprland").into_iter().map(doc).collect();
        let keys: Vec<&str> = apps.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, ["app:firefox", "app:wine-Programs-notepad"]);
        let ff = &apps[0];
        assert_eq!((ff.title.as_str(), ff.icon.as_str()), ("My Firefox", "/home/me/ff.png"));
        assert_eq!(ff.action, Action::Launch { desktop_id: "firefox.desktop".into() });
        assert_eq!(apps[1].subtitle, "Edit text\nfiles");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn names_text_and_stamp() {
        let e = parse("chromium", "[Desktop Entry]\nName=Chromium\nGenericName=Web Browser\nComment=Access the Internet\nKeywords=browser;web;\nIcon=chromium\nStartupWMClass=chromium\n", "").unwrap();
        assert_eq!(e.wm_class, "chromium");
        let d = doc(e);
        assert_eq!(d.names, ["Web Browser", "browser", "web"]);
        assert_eq!(d.subtitle, "Access the Internet");
        let ss_core::Embed::Text(t) = &d.parts[0].embed else { panic!() };
        assert_eq!(t, "title: Chromium | text: Web Browser. Access the Internet. browser, web");
        assert_eq!(d.stamp, crate::sha1(t));
    }
}
