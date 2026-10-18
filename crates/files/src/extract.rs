//! Text out of documents as lines, each with where it is, and lines grouped into parts.
use chrono::{NaiveDate, TimeDelta};
use quick_xml::events::{BytesStart, Event};
use ss_core::Locator;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

pub(crate) type Lines = Vec<(Locator, String)>;
type Zip = zip::ZipArchive<File>;

/// Every line of text in a file with its locator. None when there is no text we read
/// (source code, data and unknown types are indexed by name only).
pub(crate) fn lines(path: &Path, ext: &str) -> Option<Lines> {
    let flow = |text: String| -> Lines { text.lines().map(|l| (Locator::default(), l.to_owned())).collect() };
    Some(match ext {
        "pdf" => {
            // Lowest CPU and I/O priority, like media's tools: indexing never slows down a search.
            let out = Command::new("nice").args(["-n19", "ionice", "-c3", "timeout", "120", "pdftotext", "-q", "-enc", "UTF-8"]).arg(path).arg("-")
                .stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let pages = text.split('\x0c').enumerate();
            pages.flat_map(|(n, p)| at(Locator { page: Some(n as u32 + 1), ..Default::default() }, p)).collect()
        }
        "docx" => flow(ooxml(&read(&mut zip(path)?, "word/document.xml")?)),
        "pptx" => pptx(&mut zip(path)?)?,
        "xlsx" => xlsx(&mut zip(path)?)?,
        "odt" | "ods" | "odp" => odf(&read(&mut zip(path)?, "content.xml")?, ext),
        "epub" => epub(&mut zip(path)?)?,
        "html" | "htm" | "xhtml" => flow(html_text(&String::from_utf8_lossy(&fs::read(path).ok()?))),
        "rtf" => flow(rtf_text(&fs::read(path).ok()?)),
        _ if ext.is_empty() || crate::PROSE.contains(&ext) => {
            let mut bytes = Vec::new();
            File::open(path).ok()?.take(crate::MAX_TEXT as u64).read_to_end(&mut bytes).ok()?;
            if bytes.starts_with(b"#!") || bytes.contains(&0) {
                return None; // a script, or binary
            }
            let text = String::from_utf8_lossy(&bytes);
            text.lines().enumerate().map(|(i, l)| (Locator { line: Some(i as u32 + 1), ..Default::default() }, l.to_owned())).collect()
        }
        _ => return None,
    })
}

fn at<'a>(loc: Locator, text: &'a str) -> impl Iterator<Item = (Locator, String)> + 'a {
    text.lines().map(move |l| (loc.clone(), l.to_owned()))
}

/// Groups lines into parts of about `max` bytes. A part never spans two pages, slides,
/// sheets or chapters, starts afresh at a Markdown heading, and begins with the end of the
/// previous part (a small overlap). A part's locator is that of its first line.
pub(crate) fn chunk(lines: Lines, max: usize) -> Lines {
    let mut parts: Lines = Vec::new();
    let mut prev: Option<Locator> = None; // the last line added
    for (loc, line) in lines {
        for piece in pieces(line.trim(), max) {
            let open = prev.as_ref().filter(|p| p.page == loc.page && p.slide == loc.slide && p.sheet == loc.sheet);
            let Some((_, text)) = parts.last_mut().filter(|_| open.is_some()) else {
                if !piece.is_empty() {
                    parts.push((loc.clone(), format!("{piece}\n")));
                    prev = Some(loc.clone());
                }
                continue;
            };
            if piece.is_empty() {
                if !text.ends_with("\n\n") {
                    text.push('\n');
                }
                continue;
            }
            let heading = piece.starts_with('#');
            if text.len() + piece.len() > max || heading && text.len() > max / 4 {
                let tail = if heading { "" } else { overlap(text) }.to_owned();
                let start = if tail.is_empty() { loc.clone() } else { open.cloned().unwrap_or_default() };
                parts.push((start, if tail.is_empty() { tail } else { tail + "\n" }));
            }
            if let Some((_, text)) = parts.last_mut() {
                text.push_str(piece);
                text.push('\n');
            }
            prev = Some(loc.clone());
        }
    }
    for (_, t) in &mut parts {
        t.truncate(t.trim_end().len());
    }
    parts
}

/// A line cut into pieces of at most `max` bytes, at spaces where possible.
pub(crate) fn pieces(mut s: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    while s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        let cut = s[..cut].rfind(' ').filter(|&c| c > max / 2).unwrap_or(cut);
        out.push(&s[..cut]);
        s = s[cut..].trim_start();
    }
    out.push(s);
    out
}

/// The last ~100 bytes of a part's last line, from a word start.
fn overlap(text: &str) -> &str {
    let last = text.trim_end().rsplit('\n').next().unwrap_or("");
    let mut i = last.len().saturating_sub(100);
    while !last.is_char_boundary(i) {
        i += 1;
    }
    match last[i..].find(' ') {
        Some(s) if i > 0 => &last[i + s + 1..],
        _ => &last[i..],
    }
}

fn zip(path: &Path) -> Option<Zip> {
    zip::ZipArchive::new(File::open(path).ok()?).ok()
}

fn read(z: &mut Zip, name: &str) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    z.by_name(name).ok()?.take(64 << 20).read_to_end(&mut v).ok()?;
    Some(v)
}

/// `target` relative to the zip folder `dir`, as a zip path.
fn join(dir: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_owned();
    }
    let mut p: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for s in target.split('/') {
        match s {
            ".." => {
                p.pop();
            }
            "." | "" => {}
            s => p.push(s),
        }
    }
    p.join("/")
}

/// A part's relationships (OOXML): id → zip path of the target.
fn rels(z: &mut Zip, part: &str) -> HashMap<String, String> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let mut m = HashMap::new();
    if let Some(x) = read(z, &format!("{dir}/_rels/{file}.rels")) {
        xml(&x, |e| {
            if let X::Open(b"Relationship", a) = e {
                m.insert(attr(a, b"Id"), join(dir, &attr(a, b"Target")));
            }
        });
    }
    m
}

enum X<'a> {
    Open(&'a [u8], &'a BytesStart<'a>),
    Close(&'a [u8]),
    Text(&'a str),
}

/// Streams an XML document as opening tags (local name), closing tags and unescaped text.
/// Stops quietly at the first error, keeping what came before.
fn xml(data: &[u8], mut f: impl FnMut(X)) {
    let mut r = quick_xml::Reader::from_reader(data);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => f(X::Open(e.local_name().as_ref(), &e)),
            Ok(Event::Empty(e)) => {
                f(X::Open(e.local_name().as_ref(), &e));
                f(X::Close(e.local_name().as_ref()));
            }
            Ok(Event::End(e)) => f(X::Close(e.local_name().as_ref())),
            Ok(Event::Text(t)) => f(X::Text(&t.decode().unwrap_or_default())),
            Ok(Event::CData(t)) => f(X::Text(&t.decode().unwrap_or_default())),
            Ok(Event::GeneralRef(g)) => {
                let c = g.resolve_char_ref().ok().flatten().or_else(|| match &*g {
                    b"lt" => Some('<'),
                    b"gt" => Some('>'),
                    b"amp" => Some('&'),
                    b"apos" => Some('\''),
                    b"quot" => Some('"'),
                    _ => None,
                });
                if let Some(c) = c {
                    f(X::Text(c.encode_utf8(&mut [0; 4])));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn attr(e: &BytesStart, name: &[u8]) -> String {
    let a = e.try_get_attribute(name).ok().flatten();
    a.and_then(|a| a.unescape_value().ok().map(|v| v.into_owned())).unwrap_or_default()
}

/// WordprocessingML and DrawingML text (DOCX bodies, PPTX slides and notes): <t> runs, one line per paragraph.
fn ooxml(x: &[u8]) -> String {
    let (mut out, mut on) = (String::new(), false);
    xml(x, |e| match e {
        X::Open(b"t", _) => on = true,
        X::Close(b"t") => on = false,
        X::Text(t) if on => out.push_str(t),
        X::Open(b"tab", _) => out.push('\t'),
        X::Open(b"br", _) | X::Close(b"p") => out.push('\n'),
        _ => {}
    });
    out
}

/// Slides in presentation order (presentation.xml, not file names), each with its notes.
fn pptx(z: &mut Zip) -> Option<Lines> {
    let slides = rels(z, "ppt/presentation.xml");
    let mut ids = Vec::new();
    xml(&read(z, "ppt/presentation.xml")?, |e| {
        if let X::Open(b"sldId", a) = e {
            ids.push(attr(a, b"r:id"));
        }
    });
    let mut out = Vec::new();
    for (n, id) in ids.iter().enumerate() {
        let Some(slide) = slides.get(id) else { continue };
        let loc = Locator { slide: Some(n as u32 + 1), ..Default::default() };
        let notes = rels(z, slide).into_values().filter(|t| t.contains("notesSlide"));
        for part in std::iter::once(slide.clone()).chain(notes) {
            if let Some(x) = read(z, &part) {
                out.extend(at(loc.clone(), &ooxml(&x)));
            }
        }
    }
    Some(out)
}

/// One line per non-empty row, cells joined by " | ", for every sheet with its name. Numbers
/// in a date or time format read as 2026-10-18, 14:30 or 2026-10-18 14:30.
fn xlsx(z: &mut Zip) -> Option<Lines> {
    let mut shared = Vec::new();
    if let Some(x) = read(z, "xl/sharedStrings.xml") {
        let (mut s, mut on) = (String::new(), false);
        xml(&x, |e| match e {
            X::Open(b"t", _) => on = true,
            X::Close(b"t") => on = false,
            X::Text(t) if on => s.push_str(t),
            X::Close(b"si") => shared.push(std::mem::take(&mut s)),
            _ => {}
        });
    }
    let styles = read(z, "xl/styles.xml").map(|x| date_styles(&x)).unwrap_or_default();
    let targets = rels(z, "xl/workbook.xml");
    let (mut sheets, mut date1904) = (Vec::new(), false);
    xml(&read(z, "xl/workbook.xml")?, |e| match e {
        X::Open(b"sheet", a) => sheets.push((attr(a, b"name"), attr(a, b"r:id"))),
        X::Open(b"workbookPr", a) => date1904 = matches!(&*attr(a, b"date1904"), "1" | "true"),
        _ => {}
    });
    let mut out = Vec::new();
    for (name, id) in sheets {
        let Some(x) = targets.get(&id).and_then(|p| read(z, p)) else { continue };
        let (mut row, mut cells, mut val, mut kind, mut style, mut on) = (0u32, Vec::new(), String::new(), String::new(), 0, false);
        xml(&x, |e| match e {
            X::Open(b"row", a) => row = attr(a, b"r").parse().unwrap_or(row.saturating_add(1)),
            X::Open(b"c", a) => {
                kind = attr(a, b"t");
                style = attr(a, b"s").parse().unwrap_or(0);
                val.clear();
            }
            X::Open(b"v" | b"t", _) => on = true,
            X::Close(b"v" | b"t") => on = false,
            X::Text(t) if on => val.push_str(t),
            X::Close(b"c") => {
                let v = match &*kind {
                    "s" => val.trim().parse().ok().and_then(|i: usize| shared.get(i)).cloned().unwrap_or_default(),
                    "" | "n" => styles.get(style).copied().flatten().and_then(|f| serial_date(&val, date1904, f)).unwrap_or_else(|| val.clone()),
                    _ => val.clone(),
                };
                let v = v.split_whitespace().collect::<Vec<_>>().join(" ");
                if !v.is_empty() {
                    cells.push(v);
                }
            }
            X::Close(b"row") if !cells.is_empty() => {
                out.push((Locator { sheet: Some(name.clone()), row: Some(row), ..Default::default() }, cells.join(" | ")));
                cells.clear();
            }
            _ => {}
        });
    }
    Some(out)
}

/// For each cell style (styles.xml cellXfs, by index), the chrono format to show its numbers
/// with when its number format is a date or a time: built-in formats 14-22, or a custom code
/// with d, m or y (date) or h or s (time) outside quotes, escapes and [colours or locales].
fn date_styles(x: &[u8]) -> Vec<Option<&'static str>> {
    let (mut custom, mut styles, mut in_cells) = (HashMap::new(), Vec::new(), false);
    xml(x, |e| match e {
        X::Open(b"numFmt", a) => {
            custom.insert(attr(a, b"numFmtId"), attr(a, b"formatCode"));
        }
        X::Open(b"cellXfs", _) => in_cells = true,
        X::Close(b"cellXfs") => in_cells = false,
        X::Open(b"xf", a) if in_cells => {
            let id = attr(a, b"numFmtId");
            let (date, time) = match custom.get(&id) {
                Some(code) => {
                    let (mut plain, mut it) = (String::new(), code.chars());
                    while let Some(c) = it.next() {
                        match c {
                            '"' => _ = it.find(|&c| c == '"'),
                            '[' => _ = it.find(|&c| c == ']'),
                            '\\' | '_' | '*' => _ = it.next(), // the next character is literal or padding
                            c => plain.push(c.to_ascii_lowercase()),
                        }
                    }
                    let time = plain.contains(['h', 's']);
                    (plain.contains(['d', 'y']) || plain.contains('m') && !time, time) // with h or s, a lone m is minutes
                }
                None => match id.parse() {
                    Ok(14..=17) => (true, false),
                    Ok(18..=21) => (false, true),
                    Ok(22) => (true, true),
                    _ => (false, false),
                },
            };
            styles.push(match (date, time) {
                (true, false) => Some("%Y-%m-%d"),
                (false, true) => Some("%H:%M"),
                (true, true) => Some("%Y-%m-%d %H:%M"),
                _ => None,
            });
        }
        _ => {}
    });
    styles
}

/// A spreadsheet date serial (days since 1899-12-30, or 1904-01-01; the fraction is the time of
/// day) in `fmt`.
fn serial_date(v: &str, date1904: bool, fmt: &str) -> Option<String> {
    let (y, m, d) = if date1904 { (1904, 1, 1) } else { (1899, 12, 30) };
    let secs = TimeDelta::try_seconds((v.trim().parse::<f64>().ok()? * 86400.0).round() as i64)?;
    let t = NaiveDate::from_ymd_opt(y, m, d)?.and_hms_opt(0, 0, 0)?.checked_add_signed(secs)?;
    Some(t.format(fmt).to_string())
}

/// OpenDocument content.xml: ODT by paragraph, ODS by row with sheet names (one line per
/// non-empty row, cells joined by " | "), ODP by slide (draw:page, notes included).
fn odf(x: &[u8], ext: &str) -> Lines {
    let (mut out, mut cur, mut loc, mut row, mut body) = (Vec::new(), String::new(), Locator::default(), 0u32, false);
    let sheet = ext == "ods";
    xml(x, |e| match e {
        X::Open(b"body", _) => body = true, // styles before it hold text too (date separators)
        X::Open(b"page", _) if ext == "odp" => loc.slide = Some(loc.slide.unwrap_or(0) + 1),
        X::Open(b"table", a) if sheet => {
            loc.sheet = Some(attr(a, b"table:name"));
            row = 0;
        }
        X::Open(b"table-row", a) if sheet => {
            loc.row = Some(row + 1);
            row = row.saturating_add(attr(a, b"table:number-rows-repeated").parse().unwrap_or(1));
        }
        X::Open(b"s" | b"line-break", _) => cur.push(' '),
        X::Open(b"tab", _) => cur.push('\t'),
        X::Text(t) if body => cur.push_str(t),
        X::Close(b"p" | b"h") if sheet => cur.push('\t'),
        X::Close(b"p" | b"h") => out.push((loc.clone(), std::mem::take(&mut cur))),
        X::Close(b"table-row") if sheet => {
            let cells: Vec<&str> = cur.split('\t').map(str::trim).filter(|c| !c.is_empty()).collect();
            if !cells.is_empty() {
                out.push((loc.clone(), cells.join(" | ")));
            }
            cur.clear();
        }
        _ => {}
    });
    out
}

/// Chapters in spine order; the locator's `page` is the chapter (spine item, 1-based).
fn epub(z: &mut Zip) -> Option<Lines> {
    let mut opf = String::new();
    xml(&read(z, "META-INF/container.xml")?, |e| {
        if let X::Open(b"rootfile", a) = e
            && opf.is_empty()
        {
            opf = attr(a, b"full-path");
        }
    });
    let dir = opf.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
    let (mut items, mut spine) = (HashMap::new(), Vec::new());
    xml(&read(z, &opf)?, |e| match e {
        X::Open(b"item", a) => {
            items.insert(attr(a, b"id"), attr(a, b"href"));
        }
        X::Open(b"itemref", a) => spine.push(attr(a, b"idref")),
        _ => {}
    });
    let mut out = Vec::new();
    for (n, id) in spine.iter().enumerate() {
        let Some(x) = items.get(id).and_then(|h| read(z, &join(&dir, h.split('#').next().unwrap_or("")))) else { continue };
        out.extend(at(Locator { page: Some(n as u32 + 1), ..Default::default() }, &html_text(&String::from_utf8_lossy(&x))));
    }
    Some(out)
}

const BLOCK: &[&str] = &[
    "p", "div", "br", "li", "tr", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6", "title", "section", "article",
    "header", "footer", "blockquote", "pre", "ul", "ol", "table", "dt", "dd", "hr", "figcaption",
];

/// Visible text of HTML: tags, comments, script and style dropped; a line per block element.
fn html_text(h: &str) -> String {
    let low = h.to_ascii_lowercase(); // same byte offsets as `h`
    let (mut out, mut i) = (String::new(), 0);
    while let Some(lt) = h[i..].find('<').map(|j| i + j) {
        push_text(&mut out, &h[i..lt]);
        let tag = &low[lt + 1..];
        let end = if tag.starts_with("!--") {
            low[lt..].find("-->").map(|e| lt + e + 3)
        } else if let Some(t) = ["script", "style"].iter().find(|t| tag.starts_with(*t)) {
            let close = low[lt..].find(&format!("</{t}")).map(|e| lt + e);
            close.and_then(|c| low[c..].find('>').map(|e| c + e + 1))
        } else {
            low[lt..].find('>').map(|e| lt + e + 1)
        };
        let Some(end) = end else { return out };
        let name: String = tag.trim_start_matches('/').chars().take_while(char::is_ascii_alphanumeric).collect();
        if BLOCK.contains(&&*name) && !out.ends_with('\n') {
            out.push('\n');
        }
        i = end;
    }
    push_text(&mut out, &h[i..]);
    out
}

/// Appends HTML text with entities decoded and runs of whitespace as one space.
fn push_text(out: &mut String, s: &str) {
    let mut rest = s;
    while !rest.is_empty() {
        let ent = rest.strip_prefix('&').and_then(|r| {
            let e = r.bytes().take(10).position(|b| b == b';')?;
            Some((entity(&r[..e])?, e + 2))
        });
        let (c, len) = ent.unwrap_or_else(|| {
            let c = rest.chars().next().unwrap_or(' ');
            (c, c.len_utf8())
        });
        rest = &rest[len..];
        if c.is_whitespace() {
            if !out.is_empty() && !out.ends_with([' ', '\n']) {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
}

fn entity(name: &str) -> Option<char> {
    if let Some(n) = name.strip_prefix('#') {
        let code = match n.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok(),
            None => n.parse().ok(),
        };
        return code.and_then(char::from_u32);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "copy" => '©',
        _ => return None,
    })
}

/// Basic RTF: text with \par as new lines, \'hh and \uN characters; font, colour and style
/// tables, pictures, document info and \* destinations dropped.
fn rtf_text(r: &[u8]) -> String {
    let mut out = String::new();
    // `skip`: depth of the group being dropped. `fallback`: the next character only stands in
    // for the \uN before it, for readers without Unicode (\uc0 says there is none).
    let (mut depth, mut skip, mut fallback, mut uc) = (0usize, usize::MAX, false, true);
    let mut i = 0;
    while let Some(&c) = r.get(i) {
        i += 1;
        let (ch, unicode) = match c {
            b'{' => {
                depth += 1;
                (None, false)
            }
            b'}' => {
                if depth == skip {
                    skip = usize::MAX;
                }
                depth = depth.saturating_sub(1);
                (None, false)
            }
            b'\r' | b'\n' => (None, false),
            b'\\' => {
                let start = i;
                while r.get(i).is_some_and(u8::is_ascii_alphabetic) {
                    i += 1;
                }
                let word = &r[start..i];
                if word.is_empty() {
                    let Some(&s) = r.get(i) else { break };
                    i += 1;
                    let ch = match s {
                        b'\'' => {
                            let hex = r.get(i..i + 2).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
                            i += 2;
                            hex.map(char::from)
                        }
                        b'*' => {
                            skip = skip.min(depth);
                            None
                        }
                        b'~' => Some(' '),
                        b'\n' | b'\r' => Some('\n'),
                        b'\\' | b'{' | b'}' => Some(char::from(s)),
                        _ => None,
                    };
                    (ch, false)
                } else {
                    let num_start = i;
                    if r.get(i) == Some(&b'-') {
                        i += 1;
                    }
                    while r.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    let num: Option<i32> = std::str::from_utf8(&r[num_start..i]).ok().and_then(|s| s.parse().ok());
                    if r.get(i) == Some(&b' ') {
                        i += 1;
                    }
                    match word {
                        b"fonttbl" | b"colortbl" | b"stylesheet" | b"info" | b"pict" | b"listtable" | b"listoverridetable" => {
                            skip = skip.min(depth);
                            (None, false)
                        }
                        b"par" | b"line" | b"row" | b"sect" | b"page" => (Some('\n'), false),
                        b"tab" | b"cell" => (Some('\t'), false),
                        b"uc" => {
                            uc = num != Some(0);
                            (None, false)
                        }
                        b"u" => (num.and_then(|n| char::from_u32(if n < 0 { n + 65536 } else { n } as u32)), true),
                        _ => (None, false),
                    }
                }
            }
            _ => (Some(char::from(c)), false),
        };
        if let Some(ch) = ch
            && depth < skip
        {
            if fallback && !unicode {
                fallback = false;
            } else {
                out.push(ch);
                fallback = unicode && uc;
            }
        }
    }
    out
}
