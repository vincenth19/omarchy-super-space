//! Timing tool: `extract FILE…` prints what doc() makes of each file;
//! `extract --crawl` crawls $HOME and lists what it found. Used for the Pi measurements.
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    for arg in std::env::args().skip(1) {
        let t = Instant::now();
        if arg == "--crawl" {
            let files = ss_files::crawl(&home);
            eprintln!("crawl: {} files and folders in {:.0} ms", files.len(), t.elapsed().as_secs_f64() * 1e3);
            files.iter().for_each(|f| println!("{}", f.display()));
            continue;
        }
        match ss_files::doc(Path::new(&arg), &home) {
            Some(d) => {
                let ms = t.elapsed().as_secs_f64() * 1e3;
                let chars: usize = d.parts.iter().map(|p| p.text.len()).sum();
                println!("{arg}: {ms:.1} ms, {} parts, {chars} bytes, icon {}", d.parts.len(), d.icon);
                for p in d.parts.iter().take(3) {
                    println!("  {:?} {:?}", p.locator, p.text.chars().take(90).collect::<String>());
                }
            }
            None => println!("{arg}: none"),
        }
    }
}
