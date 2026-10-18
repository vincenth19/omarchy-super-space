//! Prints what a source finds on this machine, one JSON object per line:
//!   cargo run --release -p ss-sources --example dump -- apps|settings|emoji|clips|history|watch [state]
//!   … --example dump -- track [state] SECONDS   (records clip sources for SECONDS)
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = std::env::var("HOME").unwrap_or_default();
    let state = PathBuf::from(args.get(1).cloned().unwrap_or(format!("{home}/.local/state/superspace")));
    let t = Instant::now();
    let docs = match args.first().map(String::as_str).unwrap_or("") {
        "apps" => ss_sources::apps(),
        "settings" => ss_sources::settings(),
        "emoji" => ss_sources::emoji(),
        "clips" => ss_sources::clips(&state),
        "history" => ss_sources::history(),
        "watch" => return ss_sources::watch_paths().iter().for_each(|p| println!("{}", p.display())),
        "track" => {
            ss_sources::track_clip_sources(&state);
            let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
            return std::thread::sleep(Duration::from_secs(secs));
        }
        _ => return eprintln!("usage: dump apps|settings|emoji|clips|history|watch|track [state] [seconds]"),
    };
    let took = t.elapsed();
    for d in &docs {
        let embed: Vec<String> = d.parts.iter().map(|p| match &p.embed {
            ss_core::Embed::Text(t) => t.clone(),
            other => format!("{other:?}"),
        }).collect();
        let row = serde_json::json!({"key": d.key, "title": d.title, "subtitle": d.subtitle, "icon": d.icon,
            "action": d.action, "names": d.names, "stamp": d.stamp, "embed": embed});
        println!("{row}");
    }
    eprintln!("{} docs in {:.1} ms", docs.len(), took.as_secs_f64() * 1000.0);
}
