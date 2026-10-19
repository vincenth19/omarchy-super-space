//! `superspace bench [PARTS]`: a synthetic index (default 50k parts) that
//! checks the instant budget (under 15 ms at the socket) and times the
//! meaning phase's vector scan. The index is kept under $SUPERSPACE_STATE/bench-N
//! so a second run measures a freshly started daemon.

use crate::Cfg;
use crate::embed::{DIM, normalize, quantize};
use crate::search;
use crate::server::{self, Daemon};
use anyhow::Result;
use ss_core::{Action, Doc, Embed, Kind, Locator, Part};
use std::io::{BufRead, BufReader, Write};
use std::time::Instant;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Zipf-ish: a few words are very common, most are rare.
fn word<'a>(vocab: &'a [String], rng: &mut Rng) -> &'a str {
    &vocab[(rng.f().powi(3) * vocab.len() as f32) as usize % vocab.len()]
}

fn vocab(rng: &mut Rng) -> Vec<String> {
    const SYL: [&str; 24] =
        ["ka", "lo", "mi", "ra", "te", "su", "no", "pi", "ve", "do", "ri", "ma", "zu", "be", "ga", "fo", "ni", "sa", "tu", "le", "an", "or", "el", "is"];
    (0..8000).map(|_| (0..2 + rng.below(3)).map(|_| SYL[rng.below(SYL.len())]).collect()).collect()
}

fn fill(d: &Daemon, parts: usize) -> Result<()> {
    let t = Instant::now();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let vocab = vocab(&mut rng);
    let (mut made, mut n, mut batch) = (0, 0, Vec::new());
    while made < parts {
        let kind = match n % 20 {
            0 => Kind::App,
            1 => Kind::Setting,
            2 | 3 => Kind::Emoji,
            4 => Kind::Clip,
            _ => Kind::File,
        };
        let np = if kind == Kind::File { 1 + rng.below(20) } else { 1 }.min(parts - made);
        let ext = if kind == Kind::File { ".pdf" } else { "" };
        let title = format!("{} {}{ext}", word(&vocab, &mut rng), word(&vocab, &mut rng));
        let names = if matches!(kind, Kind::App | Kind::Emoji) { (0..4).map(|_| word(&vocab, &mut rng).to_string()).collect() } else { Vec::new() };
        let doc_parts = (0..np)
            .map(|i| {
                let text = (0..80).map(|_| word(&vocab, &mut rng)).collect::<Vec<_>>().join(" ");
                Part { embed: Embed::Text(text.clone()), text, locator: Locator { page: Some(i as u32 + 1), ..Locator::default() } }
            })
            .collect();
        let action = Action::Open { path: format!("/bench/{n}"), locator: Locator::default() };
        let doc = Doc { key: format!("bench:{n}"), kind, title, subtitle: "~/Bench".into(), icon: "x".into(), action, stamp: String::new(), names, parts: doc_parts };
        batch.push((doc, String::new(), 0));
        made += np;
        n += 1;
        if batch.len() == 500 {
            d.store.put(std::mem::take(&mut batch))?;
        }
    }
    d.store.put(batch)?;
    loop {
        let p = d.store.pending(5000)?;
        if p.is_empty() {
            break;
        }
        let done: Vec<_> = p.iter().map(|p| (p.id, Some(quantize(&normalize((0..DIM).map(|_| rng.f() - 0.5).collect()))))).collect();
        d.store.set_vecs(&done)?;
    }
    eprintln!("bench: built {n} items, {made} parts in {:.1}s", t.elapsed().as_secs_f32());
    Ok(())
}

fn stats(mut ms: Vec<f32>) -> (f32, f32, f32) {
    ms.sort_by(f32::total_cmp);
    let at = |p: f32| ms[((ms.len() - 1) as f32 * p).round() as usize];
    (at(0.5), at(0.95), at(1.0))
}

fn rss_mb() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string()))
        .map_or("n/a (not Linux)".into(), |kb| format!("{:.0} MB", kb.parse::<f32>().unwrap_or(0.0) / 1024.0))
}

/// Builds (once) and measures; returns the worst instant p95 at the socket, in ms.
pub fn run(cfg: &Cfg, parts: usize) -> Result<f32> {
    let dir = cfg.state.join(format!("bench-{parts}"));
    let fresh = !dir.join("index.sqlite").exists();
    let bcfg = Cfg {
        state: dir.clone(),
        config: dir.join("config"),
        sock: std::env::temp_dir().join(format!("superspace-bench-{}.sock", std::process::id())),
        query_url: "http://127.0.0.1:9/v1/embeddings".into(),
        ..cfg.clone()
    };
    let t = Instant::now();
    let d = Daemon::open(bcfg)?;
    if fresh {
        fill(&d, parts)?;
    } else {
        eprintln!("bench: loaded {} parts in {:.2}s; daemon RSS {}", d.store.mem.read().unwrap().parts.len(), t.elapsed().as_secs_f32(), rss_mb());
    }
    let vocab = vocab(&mut Rng(0x9E37_79B9_7F4A_7C15));
    let queries = [
        "k".to_string(), "ka".into(), "kal".into(), vocab[0].clone(), vocab[200].clone(), vocab[6000].clone(),
        format!("{} {}", vocab[3], vocab[50]), format!("{} {} {} {}", vocab[1], vocab[9], vocab[90], vocab[900]), "zzqx".into(),
    ];
    let listener = server::bind(&d.cfg.sock)?;
    let d2 = d.clone();
    std::thread::spawn(move || server::listen(d2, listener));
    let mut s = std::os::unix::net::UnixStream::connect(&d.cfg.sock)?;
    let mut lines = BufReader::new(s.try_clone()?).lines();
    let mut worst = 0f32;
    println!("{:<34} {:>8} {:>8} {:>8} {:>8}  rows", "instant phase (ms)", "p50", "p95", "max", "inproc95");
    let mut id = 0;
    for q in &queries {
        let (mut sock, mut inproc) = (Vec::new(), Vec::new());
        let mut nrows = 0;
        for _ in 0..30 {
            id += 1;
            let t = Instant::now();
            writeln!(s, "{}", serde_json::json!({"op": "search", "id": id, "q": q}))?;
            for line in lines.by_ref() {
                let v: serde_json::Value = serde_json::from_str(&line?)?;
                if v["id"] == id && v["phase"] == "instant" {
                    sock.push(t.elapsed().as_secs_f32() * 1000.0);
                    nrows = v["rows"].as_array().map_or(0, Vec::len);
                    break;
                }
            }
            let t = Instant::now();
            search::search(&d, q, 30, None, None, false);
            inproc.push(t.elapsed().as_secs_f32() * 1000.0);
        }
        let (p50, p95, max) = stats(sock);
        worst = worst.max(p95);
        println!("{:<34} {p50:>8.2} {p95:>8.2} {max:>8.2} {:>8.2}  {nrows}", format!("{q:?}"), stats(inproc).1);
    }
    let mut rng = Rng(42);
    let qv = search::QVecs { q: quantize(&normalize((0..DIM).map(|_| rng.f() - 0.5).collect())), photo: None };
    let mut ms = Vec::new();
    for q in &queries[3..6] {
        for _ in 0..10 {
            let t = Instant::now();
            search::search(&d, q, 30, Some(&qv), None, false);
            ms.push(t.elapsed().as_secs_f32() * 1000.0);
        }
    }
    let (p50, p95, max) = stats(ms);
    println!("final phase without the embed call (ms): p50 {p50:.2} p95 {p95:.2} max {max:.2}");
    println!("worst instant p95 at the socket: {worst:.2} ms (budget 15); RSS {}", rss_mb());
    let _ = std::fs::remove_file(&d.cfg.sock);
    Ok(worst)
}
