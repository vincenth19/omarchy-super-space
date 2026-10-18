//! Which files to index, and the searchable text inside them. Owner: files agent.
//! The rules are DESIGN.md "What to index" and "Extraction".
mod extract;

use ss_core::{Action, Doc, Embed, Kind, Locator, Part};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, Metadata};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

/// Files bigger than this are indexed by name only.
const MAX_FILE: u64 = 50 << 20;
/// At most this much text is read out of one file.
const MAX_TEXT: usize = 1 << 20;
/// Only a file's first this many bytes of text get meaning (embedded parts, about 40k tokens or
/// 7 minutes of the Pi's index server); later parts are found by keyword only.
const MAX_EMBED: usize = 160_000;

/// Binary formats worth indexing (by content or by name), so the binary check skips them.
const KNOWN_BINARY: &[&str] = &[
    "pdf", "docx", "xlsx", "pptx", "odt", "ods", "odp", "odg", "epub", "doc", "xls", "ppt", "zip", "tar", "gz",
    "tgz", "xz", "zst", "bz2", "7z", "rar", "svg",
];
/// Prose read as plain text, by line. Anything else not extracted (source code, config, data) is indexed by name.
const PROSE: &[&str] = &["txt", "text", "md", "markdown", "mdown", "mkd", "rst", "org", "adoc", "asciidoc", "tex", "csv", "tsv"];
const JUNK_DIRS: &[&str] = &["node_modules", "bower_components", "__pycache__", "venv", "site-packages", "Trash", "lost+found"];
/// Temp and partial downloads, lockfiles, source maps, databases, disk images, build output.
const JUNK_EXT: &[&str] = &[
    "swp", "swo", "swx", "tmp", "temp", "part", "partial", "crdownload", "lock", "map", "db", "sqlite",
    "sqlite3", "db3", "mdb", "accdb", "kdbx", "iso", "img", "qcow2", "vdi", "vmdk", "vhd", "vhdx", "dmg", "o",
    "obj", "a", "so", "dylib", "dll", "pyc", "pyo", "class", "rlib", "rmeta",
];
const JUNK_NAMES: &[&str] = &["package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml"];
/// A folder holding one of these opts out of indexing, with everything below it.
const OPT_OUT: &[&str] = &["CACHEDIR.TAG", ".nomedia", ".noindex"];

/// Folders to crawl and watch: the XDG user folders plus ~/Work, ~/Projects and ~/Notes,
/// all recursive, then `home` itself, always last, which is top level only (crawl and
/// watch it without descending).
pub fn roots(home: &Path) -> Vec<PathBuf> {
    let dirs = fs::read_to_string(home.join(".config/user-dirs.dirs")).unwrap_or_default();
    let xdg = |key: &str| {
        let v = dirs.lines().find_map(|l| l.strip_prefix(&format!("XDG_{key}_DIR=\""))?.strip_suffix('"'))?;
        Some(PathBuf::from(v.replace("$HOME", &home.to_string_lossy())))
    };
    let defaults = [("DESKTOP", "Desktop"), ("DOCUMENTS", "Documents"), ("DOWNLOAD", "Downloads"), ("MUSIC", "Music"), ("PICTURES", "Pictures"), ("VIDEOS", "Videos")];
    let mut roots: Vec<PathBuf> = defaults.iter().map(|(k, d)| xdg(k).unwrap_or_else(|| home.join(d)))
        .chain(["Work", "Projects", "Notes"].map(|d| home.join(d)))
        .filter(|p| p != home && p.is_dir())
        .collect();
    roots.sort();
    roots.dedup();
    let all = roots.clone();
    roots.retain(|r| !all.iter().any(|o| o != r && r.starts_with(o)));
    roots.push(home.to_path_buf());
    roots
}

/// [`wanted_in`] for `$HOME`.
pub fn wanted(path: &Path) -> bool {
    wanted_in(path, &std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default())
}

/// Whether a file or folder passes the exclusion rules (names, opt-out markers, .gitignore,
/// file checks) as crawl(home) applies them: only to folders from `path` up to its root, so a
/// home or root inside a dot folder works. It does not check that the path lies under a root.
pub fn wanted_in(path: &Path, home: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else { return false };
    let root = root_of(path, home);
    let below = path.strip_prefix(&root).unwrap_or(path);
    if meta.file_type().is_symlink() || opted_out(home) || below.components().any(|c| matches!(c, Component::Normal(n) if skip_name(n))) {
        return false;
    }
    // Each enclosing repo is asked about the path, or about the repo nested inside it, as crawl
    // sees them. Home's own repo never counts: a dotfiles repo at ~ must not hide ~/Documents.
    let mut asked = path;
    for d in path.ancestors().skip(usize::from(!meta.is_dir())).take_while(|d| d.starts_with(&root)) {
        if opted_out(d) {
            return false;
        }
        if d != asked && d != home && d.join(".git").exists() {
            let ignored = Command::new("git").arg("-C").arg(d).args(["check-ignore", "-q", "--"]).arg(asked)
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
            if ignored.is_ok_and(|s| s.code() == Some(0)) {
                return false;
            }
            asked = d;
        }
    }
    meta.is_dir() || file_ok(path, &meta)
}

/// The root `path` lies under (roots are disjoint except home, which comes last), or "" when
/// it lies under none.
fn root_of(path: &Path, home: &Path) -> PathBuf {
    roots(home).into_iter().find(|r| path.starts_with(r)).unwrap_or_default()
}

/// Every wanted file under the roots (home itself top level only).
pub fn crawl(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if opted_out(home) {
        return out;
    }
    for root in roots(home) {
        if root == home {
            for e in fs::read_dir(home).into_iter().flatten().flatten() {
                let p = e.path();
                if e.file_type().is_ok_and(|t| t.is_file()) && !skip_name(&e.file_name()) && e.metadata().is_ok_and(|m| file_ok(&p, &m)) {
                    out.push(p);
                }
            }
        } else {
            walk(&root, None, &mut out);
        }
    }
    out
}

/// Every wanted file below `dir`, a folder that was just created or moved in, as crawl(home)
/// finds them: nothing unless `dir` is inside a recursive root (home is top level only). One
/// `git ls-files` for the repo around `dir`, if any, decides what is kept inside it.
pub fn crawl_dir(dir: &Path, home: &Path) -> Vec<PathBuf> {
    let (root, mut out) = (root_of(dir, home), Vec::new());
    if root != home && wanted_in(dir, home) {
        let repo = dir.ancestors().skip(1).take_while(|d| d.starts_with(&root)).find(|d| d.join(".git").exists());
        walk(dir, repo.and_then(|r| git_files(r, dir)).as_ref(), &mut out);
    }
    out
}

/// Inside a git repo `keep` holds every path git does not ignore (files and their folders).
fn walk(dir: &Path, keep: Option<&HashSet<PathBuf>>, out: &mut Vec<PathBuf>) {
    if opted_out(dir) {
        return;
    }
    let listed = if dir.join(".git").exists() { git_files(dir, dir) } else { None };
    let keep = listed.as_ref().or(keep);
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_symlink() || skip_name(&e.file_name()) || keep.is_some_and(|k| !k.contains(&p)) {
            continue;
        }
        if t.is_dir() {
            walk(&p, keep, out);
        } else if t.is_file() && e.metadata().is_ok_and(|m| file_ok(&p, &m)) {
            out.push(p);
        }
    }
}

/// Tracked and untracked-but-not-ignored files of a repo below `under`, with every folder
/// leading to them.
fn git_files(repo: &Path, under: &Path) -> Option<HashSet<PathBuf>> {
    let out = Command::new("git").arg("-C").arg(repo).args(["ls-files", "-z", "-co", "--exclude-standard", "--"]).arg(under)
        .stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success())?;
    let mut keep = HashSet::new();
    for rel in out.stdout.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let p = repo.join(OsStr::from_bytes(rel));
        for a in p.ancestors().take_while(|a| *a != repo) {
            if !keep.insert(a.to_path_buf()) {
                break;
            }
        }
    }
    Some(keep)
}

fn skip_name(name: &OsStr) -> bool {
    let n = name.to_string_lossy();
    n.starts_with('.') || JUNK_DIRS.contains(&&*n)
}

fn opted_out(dir: &Path) -> bool {
    OPT_OUT.iter().any(|m| dir.join(m).exists())
}

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// The checks on a file itself: junk names, then binary (NUL in the first 8000 bytes) and
/// minified (average line over 110 bytes; not applied to prose, whose paragraphs are long lines).
fn file_ok(path: &Path, meta: &Metadata) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let ext = ext(path);
    let junk = name.ends_with('~') || name.starts_with("~$") || name.ends_with(".min.js") || name.ends_with(".min.css")
        || JUNK_NAMES.contains(&&*name) || JUNK_EXT.contains(&&*ext)
        || ["-journal", "-wal", "-shm"].iter().any(|s| ext.ends_with(s));
    if junk || !meta.is_file() {
        return false;
    }
    if ss_media::is_media(path) || KNOWN_BINARY.contains(&&*ext) {
        return true;
    }
    let mut head = Vec::new();
    if fs::File::open(path).and_then(|f| f.take(8000).read_to_end(&mut head)).is_err() || head.contains(&0) {
        return false;
    }
    let lines = head.iter().filter(|&&b| b == b'\n').count() + usize::from(head.last().is_some_and(|&b| b != b'\n'));
    PROSE.contains(&&*ext) || head.len() <= 110 * lines.max(1)
}

/// The Doc for a non-media file: title, icon, action and text parts with locators.
/// Returns None for media files (ss-media handles those) and unreadable files.
pub fn doc(path: &Path, home: &Path) -> Option<Doc> {
    if ss_media::is_media(path) {
        return None;
    }
    let ext = ext(path);
    let meta = fs::symlink_metadata(path).ok().filter(|m| m.is_file())?;
    let title = path.file_name()?.to_string_lossy().into_owned();
    let names = words(&path.file_stem()?.to_string_lossy());
    let heading = names.join(" ");
    let mut lines = if meta.len() > MAX_FILE { Vec::new() } else { extract::lines(path, &ext).unwrap_or_default() };
    let mut total = 0;
    lines.retain(|(_, l)| {
        total += l.len() + 1;
        total <= MAX_TEXT
    });
    let max = if matches!(&*ext, "pdf" | "pptx" | "odp") { 3000 } else { 800 };
    let mut embedded = 0;
    let parts = extract::chunk(lines, max).into_iter().map(|(locator, text)| {
        let embed = if embedded < MAX_EMBED { Embed::Text(ss_core::doc_text(&heading, &text)) } else { Embed::None };
        embedded += text.len();
        Part { embed, text, locator }
    }).collect();
    let subtitle = match path.parent().map(|p| (p, p.strip_prefix(home))) {
        Some((_, Ok(d))) if d.as_os_str().is_empty() => "~".to_owned(),
        Some((_, Ok(d))) => format!("~/{}", d.display()),
        Some((p, Err(_))) => p.display().to_string(),
        None => String::new(),
    };
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    Some(Doc {
        key: format!("file:{}", path.strip_prefix(home).unwrap_or(path).display()),
        kind: Kind::File,
        title,
        subtitle,
        icon: icon(&ext),
        action: Action::Open { path: path.display().to_string(), locator: Locator::default() },
        // "x2": bump when extraction changes, so every file is read again.
        stamp: format!("x2-{mtime}-{}", meta.len()),
        names,
        parts,
    })
}

/// Words of a file name: split on anything not a letter or digit, and at camelCase humps.
fn words(stem: &str) -> Vec<String> {
    let (mut out, mut w, mut lower) = (Vec::new(), String::new(), false);
    for c in stem.chars() {
        if (!c.is_alphanumeric() || c.is_uppercase() && lower) && !w.is_empty() {
            out.push(std::mem::take(&mut w));
        }
        if c.is_alphanumeric() {
            w.push(c);
        }
        lower = c.is_lowercase();
    }
    out.extend((!w.is_empty()).then_some(w));
    out
}

/// The default app's desktop id (without ".desktop") for the file's MIME type, else the
/// type's generic icon from shared-mime-info (x-office-document, …), else text-x-generic.
/// Only names: the user's icon theme draws them.
fn icon(ext: &str) -> String {
    static MIME: LazyLock<(HashMap<String, String>, HashMap<String, String>)> = LazyLock::new(|| {
        let read = |f: &str| fs::read_to_string(Path::new("/usr/share/mime").join(f)).unwrap_or_default();
        let mut globs = HashMap::new();
        for l in read("globs2").lines() {
            let mut f = l.split(':').skip(1); // weight:mime:glob, highest weight first
            if let (Some(mime), Some(e)) = (f.next(), f.next().and_then(|g| g.strip_prefix("*.")))
                && !e.contains(['*', '?', '['])
                && e == e.to_lowercase()
            {
                globs.entry(e.to_owned()).or_insert_with(|| mime.to_owned());
            }
        }
        let generic = read("generic-icons").lines().filter_map(|l| l.split_once(':')).map(|(m, i)| (m.into(), i.into())).collect();
        (globs, generic)
    });
    static ICONS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
    let mime = MIME.0.get(ext).map_or(if ext.is_empty() { "text/plain" } else { "application/octet-stream" }, |m| m);
    if let Some(i) = ICONS.lock().ok().and_then(|c| c.get(mime).cloned()) {
        return i;
    }
    let app = Command::new("xdg-mime").args(["query", "default", mime]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches(".desktop").to_owned())
        .filter(|a| !a.is_empty());
    let i = app.or_else(|| MIME.1.get(mime).cloned()).unwrap_or_else(|| "text-x-generic".into());
    if let Ok(mut c) = ICONS.lock() {
        c.insert(mime.to_owned(), i.clone());
    }
    i
}
