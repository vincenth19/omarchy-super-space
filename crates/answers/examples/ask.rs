//! Try answers by hand: `cargo run -p ss-answers --example ask -- "5'11\" in cm"`.
//! With no argument, reads one query per line from stdin and prints only the
//! ones that got an answer, with the time each took. Rates come from
//! ~/.cache/superspace (fetched when stale).
use std::io::BufRead;

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let rates = ss_answers::rates(&std::path::Path::new(&home).join(".cache/superspace"));
    let tz = std::env::var("TZ").unwrap_or_else(|_| "Australia/Brisbane".into());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let ask = |q: &str| {
        let t = std::time::Instant::now();
        let a = ss_answers::answer(q, rates.as_ref(), now, &tz);
        (a, t.elapsed().as_secs_f64() * 1e6)
    };
    if let Some(q) = std::env::args().nth(1) {
        let (a, us) = ask(&q);
        println!("{} ({us:.1} µs)", serde_json::to_string_pretty(&a).unwrap());
        return;
    }
    let (mut n, mut total, mut worst) = (0, 0.0, 0.0f64);
    for q in std::io::stdin().lock().lines().map_while(Result::ok) {
        let (a, us) = ask(&q);
        (n, total, worst) = (n + 1, total + us, worst.max(us));
        if let Some(a) = a {
            println!("{q}\t{} {}\t{us:.1} µs", a.kind, a.output);
        }
    }
    eprintln!("{n} queries, mean {:.1} µs, worst {worst:.1} µs", total / n.max(1) as f64);
}
