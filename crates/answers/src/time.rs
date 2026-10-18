//! Time zones: "time in berlin", "12pm here in london", "3pm sydney in jakarta",
//! "9am pst to aest". Places: every IANA zone's city, plus places.txt
//! (countries, capitals, large cities, states), plus fixed abbreviations.
use chrono::{DateTime, NaiveDateTime, NaiveTime, Offset, TimeDelta, TimeZone};
use chrono_tz::{OffsetName, Tz};
use ss_core::Answer;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
enum Zone {
    Tz(Tz),
    /// Seconds east of UTC: "pst", "utc+8".
    Fixed(i32),
}

struct Place {
    zone: Zone,
    name: String,
}

/// Abbreviations as fixed offsets in minutes: "9am PST" means UTC-8 whatever the season.
#[rustfmt::skip]
const ABBR: &[(&str, i32)] = &[
    ("utc", 0), ("gmt", 0), ("bst", 60), ("ist", 330), ("cet", 60), ("cest", 120), ("eet", 120), ("eest", 180),
    ("wet", 0), ("west", 60), ("msk", 180), ("est", -300), ("edt", -240), ("cst", -360), ("cdt", -300),
    ("mst", -420), ("mdt", -360), ("pst", -480), ("pdt", -420), ("akst", -540), ("akdt", -480), ("hst", -600),
    ("ast", -240), ("adt", -180), ("nst", -210), ("ndt", -150), ("brt", -180), ("art", -180), ("wat", 60),
    ("cat", 120), ("eat", 180), ("sast", 120), ("gst", 240), ("pkt", 300), ("npt", 345), ("ict", 420),
    ("sgt", 480), ("hkt", 480), ("pht", 480), ("awst", 480), ("jst", 540), ("kst", 540), ("acst", 570),
    ("acdt", 630), ("aest", 600), ("aedt", 660), ("nzst", 720), ("nzdt", 780),
];

const NOW: &[&str] = &[
    "what time is it in ", "the time in ", "current time in ", "local time in ", "time now in ", "time in ",
    "time at ", "now in ", "clock in ",
];

fn places() -> &'static HashMap<String, (Tz, String)> {
    static MAP: OnceLock<HashMap<String, (Tz, String)>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for tz in chrono_tz::TZ_VARIANTS {
            if !tz.name().starts_with("Etc/") && !tz.name().contains(|c: char| c.is_ascii_digit()) {
                let city = tz.name().rsplit('/').next().unwrap_or_default().replace('_', " ");
                map.insert(city.to_lowercase(), (tz, city));
            }
        }
        for (zone, names) in include_str!("places.txt").lines().filter_map(|l| l.split_once(": ")) {
            let Ok(tz) = zone.parse::<Tz>() else { continue };
            for n in names.split(", ") {
                map.insert(n.to_lowercase(), (tz, n.to_string()));
            }
        }
        map
    })
}

fn lookup(name: &str, home: &Place) -> Option<Place> {
    let n = name.trim().trim_start_matches("the ").trim_end_matches(" time").trim();
    if ["here", "home", "local", "my time", "me"].contains(&n) {
        return Some(Place { zone: home.zone, name: home.name.clone() });
    }
    if let Some((_, m)) = ABBR.iter().find(|a| a.0 == n) {
        return Some(Place { zone: Zone::Fixed(m * 60), name: n.to_uppercase() });
    }
    // "utc+8", "gmt-5", "utc+5:30"
    if let Some(off) = n.strip_prefix("utc").or_else(|| n.strip_prefix("gmt")) {
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let (h, m) = off.get(1..)?.split_once(':').unwrap_or((off.get(1..)?, "0"));
        let (h, m) = (h.parse::<i32>().ok().filter(|h| *h <= 14)?, m.parse::<i32>().ok().filter(|m| *m < 60)?);
        let secs = (h * 3600 + m * 60) * sign;
        return off.starts_with(['+', '-']).then(|| Place { zone: Zone::Fixed(secs), name: utc_name(secs) });
    }
    places().get(n).map(|(tz, name)| Place { zone: Zone::Tz(*tz), name: name.clone() })
}

/// The offset (seconds east of UTC) and card label ("London, BST") at a UTC instant.
fn label(p: &Place, utc: &NaiveDateTime) -> (i32, String) {
    match p.zone {
        Zone::Tz(tz) => {
            let o = tz.offset_from_utc_datetime(utc);
            let secs = o.fix().local_minus_utc();
            let abbr = o.abbreviation().filter(|a| !a.starts_with(['+', '-'])).map_or_else(|| utc_name(secs), String::from);
            (secs, format!("{}, {abbr}", p.name))
        }
        Zone::Fixed(secs) if p.name == utc_name(secs) => (secs, p.name.clone()),
        Zone::Fixed(secs) => (secs, format!("{}, {}", p.name, utc_name(secs))),
    }
}

fn utc_name(secs: i32) -> String {
    let (sign, a) = (if secs < 0 { '-' } else { '+' }, secs.abs());
    match (a / 3600, a % 3600 / 60) {
        (0, 0) => "UTC".into(),
        (h, 0) => format!("UTC{sign}{h}"),
        (h, m) => format!("UTC{sign}{h}:{m:02}"),
    }
}

/// "12pm", "9:30 am", "9.30am", "15:00", "noon" at the start of `s`:
/// hour, minute, typed as 12-hour, and the rest.
fn clock(s: &str) -> Option<(u32, u32, bool, &str)> {
    for (w, h) in [("noon", 12), ("midday", 12), ("midnight", 0)] {
        if let Some(r) = s.strip_prefix(w) {
            return Some((h, 0, true, r));
        }
    }
    let n = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let h: u32 = s.get(..n).filter(|d| (1..=2).contains(&d.len()))?.parse().ok()?;
    let mut r = &s[n..];
    let mut m = None;
    if let Some(mm) = r.strip_prefix([':', '.']).and_then(|x| x.get(..2)).filter(|x| x.bytes().all(|c| c.is_ascii_digit())) {
        m = mm.parse::<u32>().ok();
        r = &r[3..];
    }
    let colon = m.is_some() && s.as_bytes()[n] == b':';
    let t = r.trim_start();
    let ampm = ["am", "a.m.", "pm", "p.m."].iter().find(|x| t.starts_with(*x)).copied();
    if let Some(x) = ampm {
        r = &t[x.len()..];
    }
    let m = m.unwrap_or(0);
    if !(r.is_empty() || r.starts_with(' ')) || m > 59 {
        return None;
    }
    match ampm {
        Some(x) if (1..=12).contains(&h) => Some((h % 12 + if x.starts_with('p') { 12 } else { 0 }, m, true, r)),
        None if colon && h < 24 => Some((h, m, false, r)),
        _ => None,
    }
}

pub fn answer(q: &str, now_unix: i64, home_tz: &str) -> Option<Answer> {
    let home = match home_tz.parse::<Tz>() {
        Ok(tz) => Place { zone: Zone::Tz(tz), name: home_tz.rsplit('/').next().unwrap_or(home_tz).replace('_', " ") },
        Err(_) => Place { zone: Zone::Fixed(0), name: "UTC".into() },
    };
    let now = DateTime::from_timestamp(now_unix, 0)?.naive_utc();
    // "time in berlin", "now in tokyo", "london time"
    let named = NOW.iter().find_map(|p| q.strip_prefix(p)).or_else(|| q.strip_suffix(" time now")).or_else(|| q.strip_suffix(" time"));
    if let Some(to) = named.and_then(|n| lookup(n, &home)) {
        return Some(card(&home, &to, now, false));
    }
    // "12pm here in london", "3pm sydney in jakarta", "9am pst to aest", "3pm london"
    let (h, m, twelve, rest) = clock(q)?;
    let rest = rest.trim();
    let rest = ["here ", "home ", "local "].iter().find_map(|p| rest.strip_prefix(p)).unwrap_or(rest);
    let padded = format!(" {rest}");
    let cut = [" in ", " to "].iter().filter_map(|c| padded.rfind(c).map(|i| (i, c.len()))).max();
    let (from, to) = match cut {
        Some((i, len)) => {
            let left = padded[..i].trim();
            let left = left.strip_prefix("in ").or_else(|| left.strip_prefix("at ")).unwrap_or(left);
            let from = if left.is_empty() { lookup("here", &home)? } else { lookup(left, &home)? };
            (from, lookup(&padded[i + len..], &home)?)
        }
        None => (lookup(rest, &home)?, lookup("here", &home)?),
    };
    // That clock time today, in the place it was given for.
    let (off, _) = label(&from, &now);
    let day = (now + TimeDelta::seconds(off.into())).date();
    let local = day.and_time(NaiveTime::from_hms_opt(h, m, 0)?);
    let utc = match from.zone {
        Zone::Tz(tz) => tz.from_local_datetime(&local).earliest()?.naive_utc(),
        Zone::Fixed(secs) => local - TimeDelta::seconds(secs.into()),
    };
    Some(card(&from, &to, utc, twelve))
}

fn card(from: &Place, to: &Place, utc: NaiveDateTime, twelve: bool) -> Answer {
    let ((a, input_label), (b, output_label)) = (label(from, &utc), label(to, &utc));
    let at = |secs: i32| utc + TimeDelta::seconds(secs.into());
    let style = if twelve { "%-I:%M %p" } else { "%H:%M" };
    let output = at(b).format(style).to_string();
    let d = b - a;
    let gap = match (d.abs() / 3600, d.abs() % 3600 / 60) {
        (0, 0) => String::new(),
        (h, 0) => format!("{h} h"),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m} min"),
    };
    let side = if d > 0 { "ahead of" } else { "behind" };
    let relation = if d == 0 { format!("Same time as {}", from.name) } else { format!("{} is {gap} {side} {}", to.name, from.name) };
    Answer {
        kind: "time".into(),
        input: at(a).format(style).to_string(),
        input_label,
        copy: output.clone(),
        output,
        output_label,
        detail: format!("{} · {relation}", at(b).format("%a %-d %b")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_listed_zone_exists() {
        for line in include_str!("places.txt").lines() {
            let (zone, _) = line.split_once(": ").unwrap_or_else(|| panic!("bad line {line:?}"));
            assert!(zone.parse::<chrono_tz::Tz>().is_ok(), "unknown zone {zone}");
        }
        assert!(super::places().len() > 1000, "{} places", super::places().len());
    }
}
