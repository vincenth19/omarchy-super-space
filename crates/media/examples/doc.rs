//! What ss-media makes of files, and how long it takes:
//! `cargo run --release -p ss-media --example doc -- CACHE_DIR FILE…`
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let mut args = std::env::args().skip(1);
    let cache = PathBuf::from(args.next().expect("usage: doc CACHE_DIR FILE…"));
    for file in args {
        let start = Instant::now();
        let Some(d) = ss_media::doc(Path::new(&file), &home, &cache) else {
            println!("{file}: not media");
            continue;
        };
        println!("{} {:?} {:.2}s\n  {:?} · {} · icon {}\n  stamp {} names {:?}", d.key, d.kind, start.elapsed().as_secs_f32(), d.title, d.subtitle, d.icon, d.stamp, d.names);
        for p in &d.parts {
            let at = p.locator.t.map_or(String::new(), |t| format!("{t:.2}-{:.2} ", p.locator.t_end.unwrap_or(f32::NAN)));
            let embed = match &p.embed {
                ss_core::Embed::None => "keywords".to_owned(),
                ss_core::Embed::Text(t) => format!("text {} chars", t.len()),
                ss_core::Embed::Image(f) => format!("image {}", f.display()),
            };
            println!("  {at}[{embed}] {}", p.text.chars().take(100).collect::<String>().replace('\n', " / "));
        }
    }
}
