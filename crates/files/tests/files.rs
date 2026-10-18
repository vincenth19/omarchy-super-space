//! Fixtures are built here: zips for the office formats, a tiny PDF, git repos.
//! PDF and git checks run only where pdftotext / git exist (the Arch image has both).
use ss_core::{Action, Embed, Kind, Locator};
use ss_files::{crawl, crawl_dir, doc, roots, wanted_in};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ss-files-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn put(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn zipped(path: &Path, entries: &[(&str, &str)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut z = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, body) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

fn has(cmd: &str) -> bool {
    Command::new(cmd).arg("-v").output().is_ok() // it runs at all
}

fn texts(d: &ss_core::Doc) -> Vec<(Locator, String)> {
    d.parts.iter().map(|p| (p.locator.clone(), p.text.clone())).collect()
}

fn loc(page: Option<u32>, slide: Option<u32>, sheet: Option<&str>, row: Option<u32>) -> Locator {
    Locator { page, slide, sheet: sheet.map(Into::into), row, ..Default::default() }
}

#[test]
fn roots_follow_user_dirs() {
    let h = tmp("roots");
    put(&h.join(".config/user-dirs.dirs"), "XDG_DESKTOP_DIR=\"$HOME/\"\nXDG_DOCUMENTS_DIR=\"$HOME/Docs\"\nXDG_MUSIC_DIR=\"$HOME/Docs/Music\"\n");
    for d in ["Docs/Music", "Downloads", "Projects", "Videos"] {
        fs::create_dir_all(h.join(d)).unwrap();
    }
    // Desktop is home (not recursive), Music sits inside Documents, Pictures does not exist.
    assert_eq!(roots(&h), ["Docs", "Downloads", "Projects", "Videos", ""].map(|d| if d.is_empty() { h.clone() } else { h.join(d) }));
}

#[test]
fn exclusion_rules_and_crawl_agree() {
    let h = tmp("rules");
    let docs = h.join("Documents");
    let long_prose = format!("{}\n\n{}\n", "word ".repeat(120), "more ".repeat(120));
    let keep: &[(&str, &[u8])] = &[
        ("Documents/notes.md", b"# Notes\nhello\n"),
        ("Documents/long-paragraphs.md", long_prose.as_bytes()),
        ("Documents/report.pdf", b"%PDF-1.4\0binary"),
        ("Documents/photo.jpg", b"\xff\xd8\0\0"),
        ("Documents/backup.zip", b"PK\x03\x04\0\0"),
        ("Documents/tool.py", b"print('hi')\n"),
        ("Documents/README", b"plain words\n"),
        ("Documents/sub/deeper/data.csv", b"a,b\n1,2\n"),
        ("top.txt", b"top level file\n"),
    ];
    let minified = "var a=1;".repeat(700);
    let skip: &[(&str, &[u8])] = &[
        ("Documents/.hidden.md", b"x"),
        ("Documents/.git-like/x.md", b"x"),
        ("Documents/node_modules/pkg/readme.md", b"x"),
        ("Documents/__pycache__/m.pyc", b"x"),
        ("Documents/venv/lib/x.txt", b"x"),
        ("Documents/a.swp", b"x"),
        ("Documents/a.md~", b"x"),
        ("Documents/~$report.docx", b"x"),
        ("Documents/movie.mp4.part", b"x"),
        ("Documents/file.crdownload", b"x"),
        ("Documents/yarn.lock", b"x"),
        ("Documents/package-lock.json", b"{}"),
        ("Documents/app.min.js", b"x"),
        ("Documents/app.js.map", b"x"),
        ("Documents/bundle.js", minified.as_bytes()),
        ("Documents/blob.bin", b"ab\0cd"),
        ("Documents/a.sqlite", b"x"),
        ("Documents/a.sqlite-wal", b"x"),
        ("Documents/disk.iso", b"x"),
        ("Documents/cache/CACHEDIR.TAG", b"Signature: 8a477f597d28d172789f06886806bc55"),
        ("Documents/cache/x.md", b"x"),
        ("Documents/private/.noindex", b""),
        ("Documents/private/deep/x.md", b"x"),
        ("Documents/nomedia/.nomedia", b""),
        ("Documents/nomedia/x.txt", b"x"),
        ("Other/deep.txt", b"not under a root"),
        (".bashrc", b"x"),
    ];
    for (p, b) in keep.iter().chain(skip) {
        put(&h.join(p), b);
    }
    std::os::unix::fs::symlink(docs.join("notes.md"), docs.join("link.md")).unwrap();
    std::os::unix::fs::symlink(h.join("Other"), docs.join("linkdir")).unwrap();

    let found: BTreeSet<PathBuf> = crawl(&h).into_iter().collect();
    let want: BTreeSet<PathBuf> = keep.iter().map(|(p, _)| h.join(p)).collect();
    assert_eq!(found, want);
    let wanted = |p: &Path| wanted_in(p, &h);
    for (p, _) in keep {
        assert!(wanted(&h.join(p)), "{p} should be wanted");
    }
    for (p, _) in skip.iter().filter(|(p, _)| !p.starts_with("Other/")) {
        assert!(!wanted(&h.join(p)), "{p} should be skipped");
    }
    assert!(!wanted(&docs.join("link.md")) && !wanted(&docs.join("linkdir")));
    assert!(wanted(&docs.join("sub")) && !wanted(&docs.join("cache")) && !wanted(&docs.join("private/deep")));
    // A new folder gets what crawl finds below it; home's own subfolders are not crawled.
    let below = |d: &str| -> BTreeSet<PathBuf> { found.iter().filter(|p| p.starts_with(h.join(d))).cloned().collect() };
    assert_eq!(crawl_dir(&docs, &h).into_iter().collect::<BTreeSet<_>>(), below("Documents"));
    assert_eq!(crawl_dir(&docs.join("sub"), &h).into_iter().collect::<BTreeSet<_>>(), below("Documents/sub"));
    assert!(crawl_dir(&h.join("Other"), &h).is_empty() && crawl_dir(&docs.join("cache"), &h).is_empty());
}

#[test]
fn home_and_roots_inside_dot_folders() {
    // A dev home under ~/.cache, an opt-out marker above it, and a Music folder outside it, also under a dot folder.
    let t = tmp("dothome");
    let (h, music) = (t.join(".cache/dev/home"), t.join(".data/Music"));
    put(&t.join(".cache/CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55");
    put(&h.join(".config/user-dirs.dirs"), format!("XDG_MUSIC_DIR=\"{}\"\n", music.display()));
    let files = ["Documents/notes.md", "Documents/deep/plan.txt", "top.txt"].map(|p| h.join(p));
    for f in files.iter().chain([&music.join("lyrics.txt")]) {
        put(f, "words\n");
    }
    put(&h.join("Documents/.hidden/x.md"), "x");
    for f in &files {
        assert!(wanted_in(f, &h), "{}", f.display());
    }
    assert!(wanted_in(&music.join("lyrics.txt"), &h) && !wanted_in(&h.join("Documents/.hidden/x.md"), &h));
    let mut want = files.to_vec();
    want.push(music.join("lyrics.txt"));
    assert_eq!(crawl(&h).into_iter().collect::<BTreeSet<_>>(), want.into_iter().collect());
    assert_eq!(crawl_dir(&h.join("Documents/deep"), &h), [h.join("Documents/deep/plan.txt")]);
    // An opt-out marker in home itself still stops everything.
    put(&h.join(".noindex"), "");
    assert!(!wanted_in(&files[0], &h) && crawl(&h).is_empty() && crawl_dir(&h.join("Documents"), &h).is_empty());
}

#[test]
fn git_repos_respect_gitignore() {
    if !has("git") {
        eprintln!("skipped: no git");
        return;
    }
    let h = tmp("git");
    let repo = h.join("Projects/app");
    let files = [
        (".gitignore", "target/\n*.log\nsecret.md\n"),
        ("README.md", "# App\nabout\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("target/out.md", "built\n"),
        ("debug.log", "log\n"),
        ("secret.md", "shh\n"),
        ("notes/todo.md", "untracked but not ignored\n"),
        ("notes/run.log", "ignored inside a kept folder\n"),
        ("vendor/inner/.gitignore", "skip.md\n"),
        ("vendor/inner/doc.md", "nested repo doc\n"),
        ("vendor/inner/skip.md", "ignored by the nested repo\n"),
    ];
    for (p, b) in files {
        put(&repo.join(p), b);
    }
    let git = |dir: &Path, args: &[&str]| {
        let ok = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
        assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    };
    git(&repo, &["init", "-q"]);
    git(&repo.join("vendor/inner"), &["init", "-q"]);
    git(&repo, &["add", "README.md", "src/main.rs", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "x"]);

    let found: BTreeSet<PathBuf> = crawl(&h).into_iter().collect();
    let want: BTreeSet<PathBuf> = ["README.md", "src/main.rs", "notes/todo.md", "vendor/inner/doc.md"].iter().map(|p| repo.join(p)).collect();
    assert_eq!(found, want);
    for (p, _) in files.iter().filter(|(p, _)| !p.ends_with(".gitignore")) {
        assert_eq!(wanted_in(&repo.join(p), &h), want.contains(&repo.join(p)), "wanted({p})");
    }
    assert!(!wanted_in(&repo.join("target"), &h) && wanted_in(&repo.join("src"), &h));
    // A folder inside a repo, or holding one, keeps what crawl keeps there.
    let dir = |d: &str| crawl_dir(&repo.join(d), &h).into_iter().collect::<BTreeSet<_>>();
    assert_eq!(dir(""), want);
    assert_eq!(dir("vendor"), BTreeSet::from([repo.join("vendor/inner/doc.md")]));
    assert_eq!(dir("notes"), BTreeSet::from([repo.join("notes/todo.md")]));
    assert!(dir("target").is_empty());
    // Source code is found by name only; prose by content.
    assert!(doc(&repo.join("src/main.rs"), &h).unwrap().parts.is_empty());
    assert_eq!(doc(&repo.join("README.md"), &h).unwrap().parts.len(), 1);
}

#[test]
fn text_file_doc() {
    let h = tmp("text");
    let path = h.join("Notes/MeetupNotes_2026-draft.md");
    let mut md = String::from("# Meetup plan\n\n");
    for i in 0..60 {
        md.push_str(&format!("Line {i} talks about the Omarchy meetup demo and the Raspberry Pi five.\n"));
        if i % 7 == 6 {
            md.push('\n');
        }
    }
    md.push_str("## Food\nPizza for twenty people.\n");
    put(&path, &md);
    let d = doc(&path, &h).unwrap();
    assert_eq!(d.key, "file:Notes/MeetupNotes_2026-draft.md");
    assert_eq!((d.kind, d.title.as_str(), d.subtitle.as_str()), (Kind::File, "MeetupNotes_2026-draft.md", "~/Notes"));
    assert_eq!(d.names, ["Meetup", "Notes", "2026", "draft"]);
    assert_eq!(d.action, Action::Open { path: path.display().to_string(), locator: Locator::default() });
    assert!(d.stamp.ends_with(&format!("-{}", md.len())) && !d.icon.is_empty());

    let src: Vec<&str> = md.lines().collect();
    assert!(d.parts.len() >= 5, "{} parts", d.parts.len());
    for (i, p) in d.parts.iter().enumerate() {
        assert!(p.text.len() <= 900, "part {i} has {} bytes", p.text.len());
        let Embed::Text(e) = &p.embed else { panic!("part {i} not embedded") };
        assert_eq!(e, &ss_core::doc_text("Meetup Notes 2026 draft", &p.text));
        // The locator is the source line the part starts in.
        let line = p.locator.line.unwrap() as usize;
        assert!(src[line - 1].contains(p.text.lines().next().unwrap()), "part {i} line {line}");
    }
    // Consecutive parts overlap a little, except where a heading starts a fresh part.
    let last = d.parts.last().unwrap();
    assert!(last.text.starts_with("## Food"), "{:?}", last.text);
    let (a, b) = (&d.parts[0].text, &d.parts[1].text);
    assert!(a.ends_with(b.lines().next().unwrap()));

    // Shebang scripts and code are names only; a README with no extension is prose.
    put(&h.join("run"), "#!/bin/sh\necho hi\n");
    put(&h.join("README"), "Read me first.\n");
    put(&h.join("lib.rs"), "pub fn f() {}\n");
    assert!(doc(&h.join("run"), &h).unwrap().parts.is_empty());
    assert!(doc(&h.join("lib.rs"), &h).unwrap().parts.is_empty());
    let r = doc(&h.join("README"), &h).unwrap();
    assert_eq!((r.subtitle.as_str(), r.parts[0].text.as_str()), ("~", "Read me first."));
    assert!(doc(&h.join("photo.JPG"), &h).is_none() && doc(&h.join("missing.md"), &h).is_none());
}

#[test]
fn size_caps() {
    let h = tmp("caps");
    let big = h.join("big.txt");
    fs::File::create(&big).unwrap().set_len(51 << 20).unwrap();
    assert!(doc(&big, &h).unwrap().parts.is_empty());
    let long = h.join("long.md");
    put(&long, "All work and no play makes a dull day.\n".repeat(80_000)); // 3.1 MB
    let d = doc(&long, &h).unwrap();
    let total: usize = d.parts.iter().map(|p| p.text.len()).sum();
    assert!(total > 1_000_000 && total < 1_200_000, "{total}");
    // Only the first 160 KB get meaning; every later part is keyword-only.
    let embedded = |p: &&ss_core::Part| matches!(p.embed, Embed::Text(_));
    let meaning: usize = d.parts.iter().take_while(embedded).map(|p| p.text.len()).sum();
    assert!((160_000..161_000).contains(&meaning), "{meaning}");
    assert!(d.parts.iter().skip_while(embedded).all(|p| matches!(p.embed, Embed::None)));
    // One huge line (a paragraph with no breaks) is cut at spaces.
    put(&long, "word ".repeat(2000));
    let d = doc(&long, &h).unwrap();
    assert!(d.parts.len() >= 12 && d.parts.iter().all(|p| p.text.len() <= 900 && p.locator.line == Some(1)));
    // Non-ASCII text never splits a character.
    put(&long, "日本語のテキスト。".repeat(400));
    assert!(doc(&long, &h).unwrap().parts.len() > 3);
}

#[test]
fn docx() {
    let h = tmp("docx");
    let p = h.join("Letter.docx");
    zipped(&p, &[("word/document.xml", r#"<?xml version="1.0"?><w:document xmlns:w="w"><w:body>
        <w:p><w:pPr><w:tabs><w:tab w:val="left"/></w:tabs></w:pPr><w:r><w:t>Dear Tom &amp; Jerry,</w:t></w:r></w:p>
        <w:p><w:r><w:t xml:space="preserve">The lease </w:t></w:r><w:r><w:t>ends in March.</w:t></w:r><w:r><w:instrText>PAGE</w:instrText></w:r></w:p>
        </w:body></w:document>"#)]);
    assert_eq!(texts(&doc(&p, &h).unwrap()), [(Locator::default(), "Dear Tom & Jerry,\nThe lease ends in March.".into())]);
}

#[test]
fn xlsx() {
    let h = tmp("xlsx");
    let p = h.join("budget.xlsx");
    zipped(&p, &[
        ("xl/workbook.xml", r#"<workbook xmlns:r="r"><sheets><sheet name="Budget" sheetId="1" r:id="rId7"/><sheet name="Travel" sheetId="2" r:id="rId8"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId7" Target="worksheets/sheet2.xml"/><Relationship Id="rId8" Target="/xl/worksheets/sheet1.xml"/></Relationships>"#),
        ("xl/sharedStrings.xml", r#"<sst><si><t>Item</t></si><si><t>Cost</t></si><si><r><t>Flight to </t></r><r><t>Bali</t></r></si></sst>"#),
        ("xl/worksheets/sheet2.xml", r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
            <row r="4"><c r="A4" t="s"><v>2</v></c><c r="B4"><f>SUM(1,2)</f><v>450</v></c><c t="s"><v>99</v></c></row>
            <row r="5"/></sheetData></worksheet>"#),
        ("xl/worksheets/sheet1.xml", r#"<worksheet><sheetData><row r="2"><c t="inlineStr"><is><t>Passport</t></is></c></row></sheetData></worksheet>"#),
    ]);
    assert_eq!(texts(&doc(&p, &h).unwrap()), [
        (loc(None, None, Some("Budget"), Some(1)), "Item | Cost\nFlight to Bali | 450".into()),
        (loc(None, None, Some("Travel"), Some(2)), "Passport".into()),
    ]);
}

#[test]
fn xlsx_dates() {
    let h = tmp("xlsxdates");
    let book = |name: &str, pr: &str, cells: &str| {
        let p = h.join(name);
        zipped(&p, &[
            ("xl/workbook.xml", &format!(r#"<workbook xmlns:r="r">{pr}<sheets><sheet name="S" r:id="rId1"/></sheets></workbook>"#)),
            ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#),
            ("xl/sharedStrings.xml", "<sst><si><t>Item</t></si></sst>"),
            ("xl/styles.xml", r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="yyyy\-mm\-dd"/><numFmt numFmtId="165" formatCode="0.0&quot; days&quot;"/>
                <numFmt numFmtId="166" formatCode="[Red]0.00"/><numFmt numFmtId="167" formatCode="[$-409]mmmm d, yyyy;@"/></numFmts>
                <cellStyleXfs><xf numFmtId="14"/></cellStyleXfs>
                <cellXfs><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="165"/><xf numFmtId="166"/><xf numFmtId="20"/><xf numFmtId="22"/><xf numFmtId="167"/></cellXfs></styleSheet>"#),
            ("xl/worksheets/sheet1.xml", &format!(r#"<worksheet><sheetData><row r="1">{cells}</row></sheetData></worksheet>"#)),
        ]);
        texts(&doc(&p, &h).unwrap())[0].1.clone()
    };
    // Built-in and custom date and time formats; quoted, escaped and [bracketed] letters are not; nor are unstyled numbers and strings.
    let cells = r#"<c s="1"><v>46313</v></c><c s="2"><v>46313.75</v></c><c s="3"><v>2.5</v></c><c s="4"><v>46313</v></c><c s="5"><v>0.6041666666666666</v></c>
        <c s="6"><v>46313.5</v></c><c s="7"><v>46313</v></c><c><v>46313</v></c><c s="1" t="s"><v>0</v></c><c s="1"><v>oops</v></c>"#;
    assert_eq!(book("a.xlsx", "", cells), "2026-10-18 | 2026-10-18 | 2.5 | 46313 | 14:30 | 2026-10-18 12:00 | 2026-10-18 | 46313 | Item | oops");
    assert_eq!(book("mac.xlsx", r#"<workbookPr date1904="1"/>"#, r#"<c s="1"><v>44851</v></c>"#), "2026-10-18");
}

#[test]
fn pptx() {
    let h = tmp("pptx");
    let p = h.join("deck.pptx");
    let slide = |t: &str| format!(r#"<p:sld xmlns:a="a" xmlns:p="p"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{t}</a:t></a:r><a:br/><a:r><a:t>second line</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#);
    let (s1, s2) = (slide("Intro"), slide("Agenda"));
    zipped(&p, &[
        ("ppt/presentation.xml", r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#),
        ("ppt/_rels/presentation.xml.rels", r#"<Relationships><Relationship Id="rId2" Target="slides/slide1.xml"/><Relationship Id="rId3" Target="slides/slide2.xml"/></Relationships>"#),
        ("ppt/slides/slide1.xml", &s1),
        ("ppt/slides/slide2.xml", &s2),
        ("ppt/slides/_rels/slide1.xml.rels", r#"<Relationships><Relationship Id="rId1" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#),
        ("ppt/notesSlides/notesSlide1.xml", r#"<p:notes xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>Say hello to the room</a:t></a:r></a:p></p:notes>"#),
    ]);
    assert_eq!(texts(&doc(&p, &h).unwrap()), [
        (loc(None, Some(1), None, None), "Agenda\nsecond line".into()),
        (loc(None, Some(2), None, None), "Intro\nsecond line\nSay hello to the room".into()),
    ]);
}

#[test]
fn opendocument() {
    let h = tmp("odf");
    let content = |body: &str| format!(r#"<office:document-content xmlns:office="o" xmlns:text="t" xmlns:table="ta" xmlns:draw="d" xmlns:presentation="p"><office:automatic-styles><number:date-style><number:text>-</number:text></number:date-style></office:automatic-styles><office:body>{body}</office:body></office:document-content>"#);
    let odt = content(r#"<office:text><text:h>Title</text:h><text:p>One<text:s/>two<text:tab/>three <text:span>span</text:span></text:p></office:text>"#);
    let ods = content(r#"<office:spreadsheet><table:table table:name="Q1"><table:table-row table:number-rows-repeated="2"><table:table-cell/></table:table-row>
        <table:table-row><table:table-cell><text:p>Rent</text:p></table:table-cell><table:table-cell table:number-columns-repeated="3"/><table:table-cell><text:p>1200</text:p></table:table-cell></table:table-row></table:table>
        <table:table table:name="Q2"><table:table-row><table:table-cell><text:p>Food</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet>"#);
    let odp = content(r#"<office:presentation><draw:page draw:name="a"><draw:frame><draw:text-box><text:p>Welcome</text:p></draw:text-box></draw:frame>
        <presentation:notes><draw:page-thumbnail/><draw:frame><draw:text-box><text:p>note one</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
        <draw:page><draw:frame><draw:text-box><text:p>Thanks</text:p></draw:text-box></draw:frame></draw:page></office:presentation>"#);
    for (name, body) in [("a.odt", &odt), ("a.ods", &ods), ("a.odp", &odp)] {
        zipped(&h.join(name), &[("mimetype", "x"), ("content.xml", body)]);
    }
    assert_eq!(texts(&doc(&h.join("a.odt"), &h).unwrap()), [(Locator::default(), "Title\nOne two\tthree span".into())]);
    assert_eq!(texts(&doc(&h.join("a.ods"), &h).unwrap()), [
        (loc(None, None, Some("Q1"), Some(3)), "Rent | 1200".into()),
        (loc(None, None, Some("Q2"), Some(1)), "Food".into()),
    ]);
    assert_eq!(texts(&doc(&h.join("a.odp"), &h).unwrap()), [
        (loc(None, Some(1), None, None), "Welcome\nnote one".into()),
        (loc(None, Some(2), None, None), "Thanks".into()),
    ]);
}

#[test]
fn epub_and_html() {
    let h = tmp("epub");
    let p = h.join("book.epub");
    zipped(&p, &[
        ("mimetype", "application/epub+zip"),
        ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("OEBPS/content.opf", r#"<package><manifest><item id="c2" href="text/c2.xhtml"/><item id="c1" href="text/c1.xhtml#top"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#),
        ("OEBPS/text/c1.xhtml", "<html><head><style>p{color:red}</style></head><body><h1>Chapter&nbsp;One</h1><p>It was a dark &amp; stormy night.</p></body></html>"),
        ("OEBPS/text/c2.xhtml", "<html><body><p>The end &#8212; or is it?</p></body></html>"),
    ]);
    assert_eq!(texts(&doc(&p, &h).unwrap()), [
        (loc(Some(1), None, None, None), "Chapter One\nIt was a dark & stormy night.".into()),
        (loc(Some(2), None, None, None), "The end — or is it?".into()),
    ]);
    let page = h.join("page.html");
    put(&page, "<!doctype html><html><head><title>Trip</title><script>var x = '<p>no</p>';</script></head>\n<body><!-- hidden --><div>Bali  in\n  <b>March</b></div><SCRIPT type=x>bad()</SCRIPT><p>Tom&rsquo;s plan &lt;3</p></body></html>");
    assert_eq!(texts(&doc(&page, &h).unwrap()), [(Locator::default(), "Trip\nBali in March\nTom’s plan <3".into())]);
}

#[test]
fn rtf() {
    let h = tmp("rtf");
    let p = h.join("memo.rtf");
    put(&p, r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}{\*\generator Writer;}\f0\fs24 Hello \b world\b0\par caf\'e9 costs \u8364? 5\par\pard {\info{\title Hidden}}Done}");
    assert_eq!(texts(&doc(&p, &h).unwrap()), [(Locator::default(), "Hello world\ncafé costs € 5\nDone".into())]);
}

/// A minimal valid PDF with one Helvetica text line per page.
fn pdf(pages: &[&str]) -> Vec<u8> {
    let n = pages.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    for (i, text) in pages.iter().enumerate() {
        let content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + 2 * i));
        objs.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
    out
}

#[test]
fn pdf_pages() {
    if !has("pdftotext") {
        eprintln!("skipped: no pdftotext");
        return;
    }
    let h = tmp("pdf");
    let p = h.join("Documents/lease.pdf");
    put(&p, pdf(&["The tenant pays rent monthly", "", "Signed by both parties"]));
    let d = doc(&p, &h).unwrap();
    assert_eq!(texts(&d), [
        (loc(Some(1), None, None, None), "The tenant pays rent monthly".into()),
        (loc(Some(3), None, None, None), "Signed by both parties".into()),
    ]);
    assert_eq!(d.subtitle, "~/Documents");
    eprintln!("pdf icon: {}", d.icon);
}

#[test]
fn broken_files_never_panic() {
    let h = tmp("broken");
    let mut half = Vec::new();
    zipped(&h.join("ok.docx"), &[("word/document.xml", "<w:document><w:body><w:p><w:r><w:t>text</w:t></w:r></w:p></w:body></w:document>")]);
    half.extend_from_slice(&fs::read(h.join("ok.docx")).unwrap()[..60]);
    for ext in ["docx", "xlsx", "pptx", "odt", "ods", "odp", "epub", "pdf", "html", "rtf", "md", ""] {
        for (i, bytes) in [&b"\xff\xfe<<garbage>>{\\rtf1 \\u-"[..], &half, b"<a><b>unclosed &bogus; &#xFFFFFFFF; <script>"].iter().enumerate() {
            let p = h.join(format!("f{i}.{ext}"));
            put(&p, bytes);
            assert!(doc(&p, &h).is_some(), "{ext}");
        }
    }
    zipped(&h.join("bad.xlsx"), &[("xl/workbook.xml", r#"<workbook><sheets><sheet name="S" r:id="nope"/></sheets>"#), ("xl/sharedStrings.xml", "<sst><si><t>x")]);
    assert!(doc(&h.join("bad.xlsx"), &h).unwrap().parts.is_empty());
}
