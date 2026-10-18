//! Instant answers: maths, units, currency, time zones. See DESIGN.md "Answers".
//!
//! `answer` is pure (no I/O, no clock, no network) and takes microseconds.
//! Choices a user might trip on, each also stated on the card's detail line:
//! - "$", "dollar" and "bucks" are US dollars; "ton" is a metric tonne.
//! - cup, tbsp, tsp, fl oz, pint and gallon are US measures.
//! - "gb", "mb" are decimal (1 GB = 1000 MB); the binary figure is in the detail.
//! - Volume to mass ("3 cups to grams") assumes water.
//! - Money with no target goes to the home currency (from `home_tz`: AUD in
//!   Australia, USD when unknown), or from the home currency to USD.
//! - A bare quantity ("5 km", "72 f") converts to its usual partner unit only
//!   when written with digits, and a one-letter unit needs a space ("5 g", not
//!   "5g", which is a phone network). Time and data sizes need a target.
//! - Clock times answer in the style typed: "3pm" → "11:00 AM", "15:00" → "11:00";
//!   "time in berlin" answers in 24-hour time like Omarchy's clock. "3pm in
//!   london" is 3pm here shown in London; "3pm london" is London's 3pm shown here.
//!   Abbreviations are fixed offsets ("pst" is UTC-8 all year); "ist" is India.
//! - Maths cards leave both labels empty; "24/7" is answered as a division.

mod convert;
mod math;
mod time;

use ss_core::Answer;
use std::path::Path;

/// Exchange rates against the euro, with the date they were published.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Rates {
    pub date: String,
    pub eur: std::collections::HashMap<String, f64>,
}

/// The answer for a query, or None when it is not a question this crate answers.
/// `now_unix` and `home_tz` (an IANA name such as "Australia/Brisbane") make
/// time answers testable.
pub fn answer(q: &str, rates: Option<&Rates>, now_unix: i64, home_tz: &str) -> Option<Answer> {
    let q = normalize(q)?;
    time::answer(&q, now_unix, home_tz)
        .or_else(|| convert::answer(&q, rates, home_tz))
        .or_else(|| math::answer(&q))
}

const RATES_URL: &str = "https://api.frankfurter.app/latest?from=EUR";

/// Rates cached under `cache`, refreshed at most twice a day when online.
/// A stale cache costs one `curl` of up to 5 s, so call this off the search
/// path (at start and every few hours); offline it returns the cached copy.
pub fn rates(cache: &Path) -> Option<Rates> {
    let path = cache.join("rates.json");
    let cached: Option<Rates> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if cached.is_some() && age.is_some_and(|a| a.as_secs() < 12 * 3600) {
        return cached;
    }
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "5", RATES_URL])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success());
    let Some(fresh) = out.and_then(|o| parse_frankfurter(&o.stdout)) else { return cached };
    let tmp = cache.join("rates.json.tmp");
    let _ = std::fs::create_dir_all(cache)
        .and_then(|_| std::fs::write(&tmp, serde_json::to_vec(&fresh).unwrap_or_default()))
        .and_then(|_| std::fs::rename(&tmp, &path));
    Some(fresh)
}

/// `{"base":"EUR","date":"2026-10-16","rates":{"AUD":1.6171,…}}` → Rates.
fn parse_frankfurter(body: &[u8]) -> Option<Rates> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(Rates { date: v["date"].as_str()?.to_string(), eur: serde_json::from_value(v["rates"].clone()).ok()? })
}

/// Lowercase, one space between words, typographic symbols to ASCII, and
/// question wrapping ("what is", "how many X in Y", "?") removed.
fn normalize(q: &str) -> Option<String> {
    let mut s = q.to_lowercase();
    for (a, b) in [
        ("×", "*"), ("÷", "/"), ("−", "-"), ("–", "-"), ("’", "'"), ("‘", "'"), ("′", "'"),
        ("”", "\""), ("“", "\""), ("″", "\""), ("²", "^2"), ("³", "^3"), ("→", " to "),
        ("->", " to "), ("π", "pi"), ("√", "sqrt "), ("º", "°"),
    ] {
        s = s.replace(a, b);
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = s.trim_matches(['?', '=', ' ']);
    for p in ["what is ", "what's ", "whats ", "how much is ", "how much ", "calculate ", "calc ", "convert "] {
        s = s.strip_prefix(p).unwrap_or(s);
    }
    let s = match s.strip_prefix("how many ").and_then(|r| r.split_once(" in ")) {
        Some((unit, from)) => format!("{from} to {unit}"),
        None => s.to_string(),
    };
    (!s.is_empty() && s.len() <= 120).then_some(s)
}

/// `x` to `sig` significant digits, with thousands separators: 4,046.86.
fn num(x: f64, sig: i32) -> String {
    let a = x.abs();
    if a != 0.0 && !(1e-9..1e15).contains(&a) {
        let s = format!("{:.*e}", (sig - 1) as usize, x);
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        return format!("{}e{e}", trim(m));
    }
    let dec = if a == 0.0 { 0 } else { (sig - 1 - a.log10().floor() as i32).clamp(0, 12) as usize };
    group(&trim(&format!("{x:.dec$}")))
}

/// `x` with exactly `dec` decimals and thousands separators: 1,650,000.00.
fn fixed(x: f64, dec: usize) -> String {
    group(&format!("{x:.dec$}"))
}

fn trim(s: &str) -> String {
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s };
    if s == "-0" { "0".into() } else { s.into() }
}

fn group(s: &str) -> String {
    let (sign, s) = s.strip_prefix('-').map_or(("", s), |r| ("-", r));
    let (int, frac) = s.split_once('.').map_or((s, ""), |(i, f)| (i, f));
    let mut out = String::from(sign);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

/// The card's copy text: the shown number without separators.
fn plain(s: &str) -> String {
    s.replace(',', "")
}

#[cfg(test)]
mod tests {
    #[test]
    fn number_format() {
        assert_eq!(super::num(4046.8564224, 6), "4,046.86");
        assert_eq!(super::num(0.1 + 0.2, 10), "0.3");
        assert_eq!(super::num(-1234567.0, 6), "-1,234,567");
        assert_eq!(super::num(2f64.powi(64), 10), "1.844674407e19");
        assert_eq!(super::num(0.000123456789, 6), "0.000123457");
        assert_eq!(super::fixed(1650000.0, 2), "1,650,000.00");
    }

    #[test]
    fn frankfurter_body_and_fresh_cache() {
        let body = br#"{"amount":1.0,"base":"EUR","date":"2026-10-16","rates":{"AUD":1.6171,"USD":1.1253}}"#;
        let r = super::parse_frankfurter(body).unwrap();
        assert_eq!((r.date.as_str(), r.eur["AUD"]), ("2026-10-16", 1.6171));
        // A fresh cache is returned as is, without touching the network.
        let dir = std::env::temp_dir().join(format!("ss-answers-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rates.json"), serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(super::rates(&dir).unwrap().eur["USD"], 1.1253);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
