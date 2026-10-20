use super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

#[test]
fn media_by_extension() {
    for f in ["a.JPG", "b.png", "c.heic", "d.mp4", "e.MKV", "f.mp3", "g.flac", "h.opus"] {
        assert!(is_media(Path::new(f)), "{f}");
    }
    for f in ["a.ts", "b.txt", "c.pdf", "noext", ".mp3x"] {
        assert!(!is_media(Path::new(f)), "{f}");
    }
}

#[test]
fn lrc_lines() {
    let text = "[ar:Someone]\n[ti:Song]\n[00:01.00]Hello <00:01.50>world\n[00:03.00][01:10.5]Chorus  line\n[Chorus]\n\nplain words\n";
    assert_eq!(
        lrc(text),
        vec![
            (Some(1.0), "Hello world".into()),
            (Some(3.0), "Chorus line".into()),
            (Some(70.5), "Chorus line".into()),
            (None, "plain words".into()),
        ]
    );
    assert_eq!(clock("02:03.25"), Some(123.25));
    assert_eq!(clock("ar:x"), None);
}

#[test]
fn whisper_noise_is_dropped() {
    assert_eq!(clean(" [Music] Hello (laughs) world ♪ "), "Hello world");
    assert_eq!(clean("[BLANK_AUDIO]"), "");
}

#[test]
fn transcript_windows() {
    let segs = [(0.0, 4.0, "a"), (4.0, 9.0, "b"), (9.0, 14.0, "c"), (14.0, 15.0, "..."), (14.0, 20.0, "d"), (20.0, 50.0, "e")];
    let w = windows(segs.map(|(a, b, t)| (a, b, t.to_string())));
    assert_eq!(w, vec![(0.0, 14.0, "a b c".into()), (14.0, 20.0, "d".into()), (20.0, 50.0, "e".into())]);
}

#[test]
fn small_helpers() {
    assert_eq!(thumb(Path::new("/c/f0003.jpg")), PathBuf::from("/c/t0003.jpg"));
    assert_eq!(cut("héllo", 2), "hé");
    assert_eq!(cut("hi", 5), "hi");
    assert_eq!(syncsafe(&[0, 0, 2, 1]), 257);
    assert_eq!([length(42.4), length(192.0), length(3723.0)], ["0:42", "3:12", "1:02:03"]);
    assert!(["Music/Voice Memos", "Podcasts/Hard Fork", "Call Recordings", "Spoken Word"].iter().all(|s| speech(s)));
    assert!(!["Music/Summer Memories", "Music/Voices of Africa", "Music/Recorded Live"].iter().any(|s| speech(s)));
}

#[test]
fn track_numbers_and_days() {
    for (stem, want) in [
        ("03 - Keys on the Counter", "Keys on the Counter"),
        ("1-03. Ferry", "Ferry"),
        ("07_Rain", "Rain"),
        ("101 Dalmatians Theme", "Dalmatians Theme"),
        ("7 Rings", "7 Rings"),
        ("2026-10-05 meetup budget", "2026-10-05 meetup budget"),
        ("Kitchen Radio - Ferry to the City", "Kitchen Radio - Ferry to the City"),
        ("01", "01"),
    ] {
        assert_eq!(untracked(stem), want, "{stem}");
    }
    let year = 1970 + std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() / 31_556_952;
    assert_eq!(day(&format!("{year}-10-05 meetup budget.m4a")).as_deref(), Some("5 Oct"));
    assert_eq!(day("2019-03-07T10:22:31.000000Z").as_deref(), Some("7 Mar 2019"));
    for s in ["2019-13-07 x", "2019-03-00", "20190307", "memo 2019-03-07", "2019-0é-07", ""] {
        assert_eq!(day(s), None, "{s}");
    }
}

/// An ID3v2 tag (version 3 or 4) holding `frames` (id, body).
fn id3(version: u8, frames: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let size = |n: usize, safe: bool| -> [u8; 4] {
        if safe { [(n >> 21) as u8 & 0x7f, (n >> 14) as u8 & 0x7f, (n >> 7) as u8 & 0x7f, n as u8 & 0x7f] } else { (n as u32).to_be_bytes() }
    };
    let mut body = Vec::new();
    for (id, b) in frames {
        body.extend_from_slice(*id);
        body.extend(size(b.len(), version == 4));
        body.extend([0, 0]);
        body.extend(b);
    }
    body.extend([0; 16]); // padding
    let mut tag = b"ID3".to_vec();
    tag.extend([version, 0, 0]);
    tag.extend(size(body.len(), true));
    tag.extend(body);
    tag
}

fn utf16(s: &str) -> Vec<u8> {
    let mut b = vec![0xff, 0xfe];
    b.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
    b.extend([0, 0]);
    b
}

fn sylt_frame(enc: u8, lines: &[(&str, u32)]) -> Vec<u8> {
    let text = |s: &str| if enc == 1 { utf16(s) } else { [s.as_bytes(), &[0]].concat() };
    let mut b = vec![enc, b'e', b'n', b'g', 2, 1];
    b.extend(text(""));
    for (s, ms) in lines {
        b.extend(text(s));
        b.extend(ms.to_be_bytes());
    }
    b
}

#[test]
fn sylt_lines_and_karaoke() {
    let b = sylt_frame(1, &[("First line", 1000), ("Second línea", 2500)]);
    assert_eq!(sylt_lines(&b), vec![(1.0, "First line".into()), (2.5, "Second línea".into())]);
    let k = sylt_frame(3, &[("\nHel", 1000), ("lo", 1200), ("\nWorld", 3000)]);
    assert_eq!(sylt_lines(&k), vec![(1.0, "Hello".into()), (3.0, "World".into())]);
    let ms_frames = [1u8, b'e', b'n', b'g', 1, 1, 0, 0];
    assert!(sylt_lines(&ms_frames).is_empty(), "MPEG-frame times are not supported");
}

// ---- with ffmpeg: real files, the cache ----

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("ss-media-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("home/Videos")).unwrap();
        fs::create_dir_all(dir.join("home/Music")).unwrap();
        fs::create_dir_all(dir.join("home/Pictures")).unwrap();
        Scratch(dir)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn cache(&self) -> PathBuf {
        self.0.join("cache")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ffmpeg(args: &[&str], out: &Path) {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).arg(out).status().unwrap().success();
    assert!(ok, "ffmpeg {args:?}");
}

fn width(image: &Path) -> u32 {
    let out = Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=width", "-of", "csv=p=0"]).arg(image).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn tone(path: &Path, secs: &str) {
    ffmpeg(&["-f", "lavfi", "-i", &format!("sine=frequency=440:duration={secs}"), "-write_id3v2", "0", "-c:a", "libmp3lame"], path);
}

#[test]
fn video_frames_thumbs_cache_and_cleanup() {
    let s = Scratch::new("video");
    let path = s.home().join("Videos/clip.mp4");
    // 10 s of test pattern, then a cut to 3 s of plain blue, with a tone.
    ffmpeg(
        &[
            "-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30:d=10",
            "-f", "lavfi", "-i", "color=c=blue:s=1280x720:r=30:d=3",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=13",
            "-filter_complex", "[0:v][1:v]concat=n=2:v=1[v]", "-map", "[v]", "-map", "2:a",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac",
        ],
        &path,
    );
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    let gap = frame_gap(13.0);
    assert_eq!((d.key.as_str(), d.kind, d.title.as_str(), d.subtitle.as_str()), ("file:Videos/clip.mp4", Kind::Video, "clip.mp4", "~/Videos"));
    assert!(d.stamp.ends_with(&format!("-v3-g{gap}-{}", frame_gap(f32::INFINITY))), "{}", d.stamp);
    let times: Vec<(f32, f32)> = d.parts.iter().map(|p| (p.locator.t.unwrap(), p.locator.t_end.unwrap())).collect();
    println!("frames {times:?}");
    assert_eq!(times[0].0, 0.0);
    assert!(times.windows(2).all(|w| w[1].0 - w[0].0 <= gap + 0.1 && w[0].1 == w[1].0), "every {gap} s, contiguous: {times:?}");
    assert!(times.iter().any(|t| (10.0..10.5).contains(&t.0)), "the cut to blue at 10 s: {times:?}");
    assert!((times.last().unwrap().1 - 13.0).abs() < 0.1, "last frame lasts to the end");
    for p in &d.parts {
        let Embed::Image(f) = &p.embed else { panic!("frame part") };
        assert!(width(f) <= 448 && width(&thumb(f)) <= 160);
    }
    assert_eq!(d.icon, thumb(match &d.parts[0].embed { Embed::Image(f) => f, _ => unreachable!() }).display().to_string());

    // Unchanged: read back from parts.json, same parts.
    let t = Instant::now();
    let again = doc(&path, &s.home(), &s.cache()).unwrap();
    assert!(t.elapsed().as_millis() < 100, "cached doc took {:?}", t.elapsed());
    assert_eq!((again.stamp.as_str(), again.parts.len()), (d.stamp.as_str(), d.parts.len()));

    // Changed: new frames, the old ones are gone.
    let old = match &d.parts[0].embed { Embed::Image(f) => f.parent().unwrap().to_owned(), _ => unreachable!() };
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=5", "-c:v", "libx264", "-pix_fmt", "yuv420p"], &path);
    let new = doc(&path, &s.home(), &s.cache()).unwrap();
    assert_ne!(new.stamp, d.stamp);
    assert!(!old.exists(), "old frames removed");
    assert_eq!(new.parts.len(), (5.0 / gap).ceil() as usize, "one per {gap} s of 5 s");

    forget(&path, &s.cache());
    assert!(!s.cache().join("media").read_dir().unwrap().any(|_| true), "forget empties the cache");
}

#[test]
fn frames_at_every_cut_but_few() {
    let s = Scratch::new("gap");
    let times = |video: &Path, secs: f32, gap: f32| {
        let dir = s.0.join(format!("{secs}-{gap}"));
        fs::create_dir_all(&dir).unwrap();
        frames(video, &dir, secs, gap, Duration::from_secs(120)).unwrap().into_iter().map(|f| f.1).collect::<Vec<f32>>()
    };
    // Like the demo's weekend clip (demo/corpus/truth.json): scenes from 0, 10, 30 and 45 s to 59 s.
    let scenes = s.home().join("Videos/scenes.mp4");
    let colour = "geq=r='255*(lt(T,10)+gte(T,30)*lt(T,45))':g='255*gte(T,10)*lt(T,30)':b='255*gte(T,45)'";
    ffmpeg(&["-f", "lavfi", "-i", &format!("color=s=160x90:r=10:d=59,{colour}"), "-c:v", "libx264", "-pix_fmt", "yuv420p"], &scenes);
    // Ten minutes of red and blue swapping every half second: a cut every 0.5 s.
    let cuts = s.home().join("Videos/cuts.mp4");
    let flip = "geq=r='255*lt(mod(T,1),0.5)':g=0:b='255*gte(mod(T,1),0.5)'";
    ffmpeg(&["-f", "lavfi", "-i", &format!("color=s=64x48:r=4:d=600,{flip}"), "-c:v", "libx264", "-pix_fmt", "yuv420p"], &cuts);
    for gap in [4.0, 8.0] {
        let t = times(&scenes, 59.0, gap);
        println!("gap {gap}: {t:?}");
        assert!([10.0, 30.0, 45.0].iter().all(|&c| t.iter().any(|&f| (c..c + 0.5).contains(&f))), "a frame at each cut: {t:?}");
        assert!(t[0] == 0.0 && t.windows(2).all(|w| w[1] - w[0] <= gap + 0.2) && t.len() as f32 <= 2.0 * 59.0 / gap, "{t:?}");
        // Cuts beyond two per gap and minutes past eight do not add frames: at most 960 / gap.
        let n = times(&cuts, 600.0, gap).len() as f32;
        assert!((480.0 / gap..=960.0 / gap).contains(&n), "{n} frames at gap {gap}");
    }
}

#[test]
fn short_videos_keep_4_s_frames() {
    // At 8 s the demo's 59 s weekend clip lost its best golden retriever frame (0:49).
    let few_cores = std::thread::available_parallelism().unwrap().get() <= 4;
    assert_eq!([frame_gap(59.0), frame_gap(120.0), frame_gap(121.0)], [4.0, 4.0, if few_cores { 8.0 } else { 4.0 }]);
}

#[test]
fn tools_run_idle_with_names_as_they_are() {
    let out = run(Command::new("sh").args(["-c", "echo \"$0\"; ps -o ni= -p $$; cat /proc/self/cgroup 2>/dev/null; true", "${HOME}$$"]), Duration::from_secs(10)).unwrap();
    let out = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    assert_eq!(lines[..2], ["${HOME}$$", "19"], "{out}");
    if scoped() {
        let cg = lines.iter().find_map(|l| l.strip_prefix("0::")).unwrap();
        assert!(cg.ends_with(".scope"), "{cg}");
        assert_eq!(fs::read_to_string(format!("/sys/fs/cgroup{cg}/cpu.idle")).unwrap().trim(), "1", "idle scope {cg}");
    }
    println!("scoped {} {out}", scoped());
}

/// Mean luma of a picture, 0-255.
fn luma(image: &Path) -> f32 {
    let out = Command::new("ffmpeg").arg("-i").arg(image).args(["-vf", "signalstats,metadata=print:key=lavfi.signalstats.YAVG", "-f", "null", "-"]).output().unwrap();
    String::from_utf8_lossy(&out.stderr).split_once("YAVG=").unwrap().1.split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
fn black_frames_are_skipped() {
    let s = Scratch::new("black");
    // Like the demo's weekend clip at 30 s: a cut to black that fades in, here at 0 and 6 s.
    let path = s.home().join("Videos/fades.mp4");
    ffmpeg(
        &[
            "-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=6,fade=in:d=1.5",
            "-f", "lavfi", "-i", "smptebars=s=640x360:r=30:d=6,fade=in:d=1.5",
            "-filter_complex", "[0:v][1:v]concat=n=2:v=1[v]", "-map", "[v]", "-c:v", "libx264", "-pix_fmt", "yuv420p",
        ],
        &path,
    );
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    let frames: Vec<(f32, f32)> = d.parts.iter().map(|p| (p.locator.t.unwrap(), match &p.embed { Embed::Image(f) => luma(f), _ => panic!("frame part") })).collect();
    println!("(time, luma) {frames:?}");
    assert!(frames.iter().all(|f| f.1 > 15.0), "no near-black frame: {frames:?}");
    assert!((0.1..1.5).contains(&frames[0].0), "the first frame that shows something: {frames:?}");
    assert!(frames.iter().any(|f| (6.1..7.5).contains(&f.0)), "the cut at 6 s, once it shows something: {frames:?}");
    // Black throughout: no frames, and that result is kept.
    let black = s.home().join("Videos/black.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=black:s=320x240:r=30:d=3", "-c:v", "libx264", "-pix_fmt", "yuv420p"], &black);
    let d = doc(&black, &s.home(), &s.cache()).unwrap();
    assert!(d.parts.is_empty() && d.icon == "video-x-generic" && load(&results(&black, &s.cache(), &d.stamp)).is_some());
}

#[test]
fn photo_scaled_with_thumb() {
    let s = Scratch::new("photo");
    let path = s.home().join("Pictures/holiday.jpg");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=1600x1200:d=1", "-frames:v", "1"], &path);
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    assert_eq!(d.kind, Kind::Image);
    assert_eq!(d.parts.len(), 1, "a camera-style JPEG gets no OCR");
    let Embed::Image(f) = &d.parts[0].embed else { panic!("image part") };
    assert_eq!((width(f), width(&thumb(f))), (448, 160));
    assert_eq!(d.icon, thumb(f).display().to_string());
    // Small pictures are not enlarged.
    let small = s.home().join("Pictures/icon.jpg");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=100x80:d=1", "-frames:v", "1"], &small);
    let Embed::Image(f) = &doc(&small, &s.home(), &s.cache()).unwrap().parts[0].embed else { panic!() };
    assert_eq!(width(f), 100);
}

#[test]
fn song_with_lrc() {
    let s = Scratch::new("lrc");
    let path = s.home().join("Music/song.mp3");
    tone(&path, "6");
    fs::write(s.home().join("Music/song.lrc"), "[ti:Song]\n[00:01.00]We sang all night\n[00:03.50]Under the city lights\n").unwrap();
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    assert!(d.stamp.contains("-lrc"));
    assert_eq!(d.parts[0].text, "We sang all night\nUnder the city lights");
    assert!(matches!(&d.parts[0].embed, Embed::Text(t) if t.starts_with("title: song.mp3 | text: We sang")));
    let timed: Vec<_> = d.parts[1..].iter().map(|p| (p.text.as_str(), p.locator.t, p.locator.t_end.map(f32::round))).collect();
    assert_eq!(timed, vec![("We sang all night", Some(1.0), Some(4.0)), ("Under the city lights", Some(3.5), Some(6.0))]);
    assert!(matches!(d.parts[1].embed, Embed::None));
}

#[test]
fn song_with_id3_lyrics() {
    let s = Scratch::new("id3");
    let raw = s.0.join("raw.mp3");
    tone(&raw, "4");
    let audio = fs::read(&raw).unwrap();
    // USLT (unsynchronised), with title and artist frames: v2.3, UTF-8 text
    let uslt = [&[3u8][..], b"eng", &[0], "Rain on the window\nI miss you".as_bytes()].concat();
    let tit2 = [&[3u8][..], b"Rainy Day"].concat();
    let tpe1 = [&[3u8][..], b"The Band"].concat();
    let path = s.home().join("Music/uslt.mp3");
    fs::write(&path, [id3(3, &[(b"TIT2", tit2), (b"TPE1", tpe1), (b"USLT", uslt)]), audio.clone()].concat()).unwrap();
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    assert_eq!((d.title.as_str(), d.subtitle.as_str()), ("Rainy Day", "Song · The Band · 0:04"));
    assert_eq!(d.names, vec!["Rainy Day", "The Band", "uslt.mp3"]);
    let q = quick_doc(&path, &s.home(), &s.cache()).unwrap();
    assert_eq!((q.title, q.subtitle, q.names, q.icon), (d.title.clone(), d.subtitle.clone(), d.names.clone(), d.icon.clone()), "quick_doc reuses doc's tags");
    assert_eq!(d.parts.len(), 1);
    assert_eq!(d.parts[0].text, "Rain on the window\nI miss you");
    assert!(matches!(&d.parts[0].embed, Embed::Text(t) if t.starts_with("title: Rainy Day | text: Rain")));
    // SYLT (synchronised), v2.4, UTF-16
    let path = s.home().join("Music/sylt.mp3");
    let frame = sylt_frame(1, &[("Rain on the window", 500), ("I miss you", 2000)]);
    fs::write(&path, [id3(4, &[(b"SYLT", frame)]), audio].concat()).unwrap();
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    let timed: Vec<_> = d.parts.iter().map(|p| (p.text.as_str(), p.locator.t)).collect();
    assert_eq!(timed, vec![("Rain on the window\nI miss you", None), ("Rain on the window", Some(0.5)), ("I miss you", Some(2.0))]);
}

/// Two seconds of tone at `path` with `tags` ("artist=The Band").
fn tagged(path: &Path, tags: &[&str]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut args = vec!["-f", "lavfi", "-i", "sine=frequency=440:duration=2"];
    args.extend(tags.iter().flat_map(|t| ["-metadata", t]));
    ffmpeg(&args, path);
}

#[test]
fn songs_and_recordings() {
    let s = Scratch::new("songs");
    let year = 1970 + std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() / 31_556_952;
    let memo = format!("Music/Voice Memos/{year}-10-05 meetup budget.m4a");
    let cases = [
        // Phone recorders tag a title, and podcasts an artist and album: their folders decide.
        (memo.as_str(), &["title=meetup budget"][..], memo.rsplit('/').next().unwrap().to_owned(), "Recording · 5 Oct · 0:02"),
        ("Podcasts/Hard Fork/ep 101.mp3", &["title=Episode 101", "artist=NYT", "album=Hard Fork"], "ep 101.mp3".into(), "Recording · 0:02"),
        ("Music/Kitchen Radio/Demo Tapes/02 - Keys.mp3", &["title=Keys on the Counter", "artist=Kitchen Radio", "album=Demo Tapes"], "Keys on the Counter".into(), "Song · Kitchen Radio · 0:02"),
        ("Music/03 - Untagged Song.mp3", &[], "Untagged Song".into(), "Song · 0:02"),
        ("Downloads/mixtape.mp3", &["album=Summer"], "mixtape".into(), "Song · 0:02"),
        ("Downloads/talk.mp3", &["artist=Someone", "genre=Speech"], "talk.mp3".into(), "Recording · 0:02"),
        ("Downloads/call.m4a", &["title=Call with Sam", "creation_time=2019-03-07T10:22:31Z"], "call.m4a".into(), "Recording · 7 Mar 2019 · 0:02"),
    ];
    for (rel, tags, title, subtitle) in cases {
        let path = s.home().join(rel);
        tagged(&path, tags);
        let d = doc(&path, &s.home(), &s.cache()).unwrap();
        assert_eq!((d.title.as_str(), d.subtitle.as_str()), (title.as_str(), subtitle), "{rel}");
        assert!(matches!(&d.action, Action::Open { locator, .. } if locator.t_end.is_some_and(|e| (e - 2.0).abs() < 0.1)), "{rel}: the whole file's length");
        assert_eq!(d.names.last().is_some_and(|n| rel.ends_with(n.as_str())), title != *rel.rsplit('/').next().unwrap(), "{rel}: the file name is a name when the title differs");
    }
    // Before the slow work, the folder alone decides.
    let q = |rel: &str| {
        let path = s.home().join(rel);
        fs::write(&path, "not yet read").unwrap();
        let q = quick_doc(&path, &s.home(), &s.cache()).unwrap();
        (q.title, q.subtitle)
    };
    assert_eq!(q("Music/04 Saturday.mp3"), ("Saturday".into(), "Song".into()));
    assert_eq!(q("Music/Voice Memos/2019-03-07 app idea.m4a"), ("2019-03-07 app idea.m4a".into(), "Recording · 7 Mar 2019".into()));
    assert_eq!(q("Downloads/04 Saturday.mp3"), ("04 Saturday.mp3".into(), "Recording".into()));
}

#[test]
fn audio_without_speech_to_text() {
    let s = Scratch::new("stt");
    let path = s.home().join("Music/tone.mp3");
    tone(&path, "2");
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    assert!(!d.stamp.contains("-stt") && d.parts.is_empty() && d.icon == "audio-x-generic");
}

#[test]
fn quick_doc_names_now_and_reuses_the_cache() {
    let s = Scratch::new("quick");
    let path = s.home().join("Pictures/beach day.jpg");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=640x480:d=1", "-frames:v", "1"], &path);
    let q = quick_doc(&path, &s.home(), &s.cache()).unwrap();
    assert_eq!(
        (q.key.as_str(), q.kind, q.title.as_str(), q.subtitle.as_str(), q.icon.as_str()),
        ("file:Pictures/beach day.jpg", Kind::Image, "beach day.jpg", "~/Pictures", "image-x-generic")
    );
    assert_eq!(q.action, Action::Open { path: path.display().to_string(), locator: Locator::default() });
    assert_eq!(q.stamp, format!("quick-{}", stamp(&path, &s.home()).unwrap()));
    assert!(q.parts.is_empty() && q.names.is_empty());
    // Once doc has run on this version of the file, the quick Doc has its thumbnail.
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    let q = quick_doc(&path, &s.home(), &s.cache()).unwrap();
    assert!(q.icon == d.icon && Path::new(&q.icon).is_file() && q.parts.is_empty() && q.stamp != d.stamp);
    let t = Instant::now();
    for _ in 0..100 {
        quick_doc(&path, &s.home(), &s.cache()).unwrap();
    }
    println!("quick_doc {:?}", t.elapsed() / 100);
    assert!(t.elapsed() < Duration::from_millis(100), "quick_doc took {:?} per call", t.elapsed() / 100);
    assert!(quick_doc(&s.home().join("notes.txt"), &s.home(), &s.cache()).is_none());
}

#[test]
fn lrc_changes_the_stamp_and_names_its_song() {
    let s = Scratch::new("lrcstamp");
    let music = s.home().join("Music");
    let (song, lrc) = (music.join("Track 1.MP3"), music.join("Track 1.lrc"));
    fs::write(&song, "audio").unwrap();
    fs::write(music.join("Track 1.mp4"), "video").unwrap(); // a video is not a .lrc's owner
    let stamps: Vec<String> = [None, Some("[00:01.00]one"), Some("[00:01.00]one two"), None]
        .into_iter()
        .map(|text| {
            match text {
                Some(t) => fs::write(&lrc, t).unwrap(),
                None => drop(fs::remove_file(&lrc)),
            }
            stamp(&song, &s.home()).unwrap()
        })
        .collect();
    assert!(stamps[0] != stamps[1] && stamps[1] != stamps[2] && stamps[3] == stamps[0], "{stamps:?}");
    assert_eq!(lrc_owner(&lrc), Some(song.clone()), "also once the .lrc is deleted");
    assert_eq!(lrc_owner(&music.join("notes.lrc")), None);
    assert_eq!(lrc_owner(&song), None);
}

#[test]
fn hung_tool_is_killed_with_its_children() {
    let s = Scratch::new("hang");
    let data = s.home().join(".local/share/superspace");
    fs::create_dir_all(data.join("whisper")).unwrap();
    fs::create_dir_all(data.join("models")).unwrap();
    for m in ["ggml-base-q8_0.bin", "ggml-silero-v6.2.0.bin"] {
        fs::write(data.join("models").join(m), "").unwrap();
    }
    // A fake whisper that never ends; its child keeps the output pipes open, so only
    // killing the whole group lets doc return.
    let cli = data.join("whisper/whisper-cli");
    fs::write(&cli, "#!/bin/sh\nsleep 600 &\nsleep 600\n").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let path = s.home().join("Music/voice.mp3");
    tone(&path, "1");
    let t = Instant::now();
    let d = doc(&path, &s.home(), &s.cache()).unwrap();
    // The limit is GRACE (3 s in tests) plus 3 s per second of audio: about 6 s here.
    let took = t.elapsed();
    assert!((Duration::from_secs(5)..Duration::from_secs(15)).contains(&took), "doc took {took:?}");
    assert!(d.stamp.ends_with("-stt") && d.parts.is_empty());
    let saved = fs::read_dir(s.cache().join("media")).unwrap().flatten().flat_map(|i| fs::read_dir(i.path()).unwrap().flatten());
    assert!(!saved.into_iter().any(|v| v.path().join("parts.json").exists()), "a hung tool's result is not cached");
}
