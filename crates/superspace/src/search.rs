//! Ranking (DESIGN.md "Ranking" and "Learned scorer"): candidates from fuzzy
//! names, FTS5 keywords and vectors; one row per item; a weighted sum of
//! features, learned online from picks; per-kind cutoffs decide what is a real
//! match; the web row always last.

use crate::embed::{DIM, dot, normalize, quantize};
use crate::server::Daemon;
use crate::store::{Item, Mem, Pick, kind_name, now, part_text};
use anyhow::{Context, Result};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use rusqlite::Connection;
use serde_json::{Value, json};
use ss_core::{Action, Hit, Kind, Locator, Row, Snippet, Why};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A query vector, int8 with its scale.
pub type QVec = (Vec<i8>, f32);

/// The query's vectors: as typed, and as "a photo of …" (only while the photo feature has a weight).
#[derive(Clone)]
pub struct QVecs {
    pub q: QVec,
    pub photo: Option<QVec>,
}

/// Meaning cutoffs (cosine) per kind; "frame" is any part embedded from a
/// picture (photos, video frames); "name" is the least fuzzy name score that
/// counts (1.0 = the query itself; word-start matches score 0.87+, scattered
/// letters 0.7-0.79). Measured with EmbeddingGemma 2 Q8_0 on a small corpus:
/// files matching 0.66-0.79, unrelated 0.45-0.625; Omarchy's short app and
/// setting labels sit near everything (unrelated up to 0.69, matches 0.67-0.78);
/// photos 0.72-0.80 against 0.50-0.62. Tuned by tests/tune on the demo corpus (tests/tuned.toml,
/// 18 Oct); `[cutoffs]` in preferences.toml overrides any of them.
pub const CUTOFFS: &[(&str, f32)] = &[
    ("app", 0.70), ("setting", 0.70), ("file", 0.68), ("image", 0.67), ("video", 0.705), ("audio", 0.665),
    ("clip", 0.715), ("emoji", 0.705), ("history", 0.64), ("frame", 0.68), ("name", 0.80),
];

pub const NF: usize = 23;
/// The scorer's features, in weight order; weights.json uses these names.
/// "photo": how much higher a picture's meaning match is against "a photo of
/// <q>" (with photo = meaning's weight, a picture scores the better of the two).
/// "generic": how much more a picture resembles "a photo", "an image", … than
/// the query's other pictures do (a negative weight sinks pictures that match
/// everything). Both are 0 by default: on the demo corpus with EmbeddingGemma 2
/// neither widened the margin of the right photo, and photo costs an extra
/// query embedding.
pub const FEATURES: [&str; NF] = [
    "name", "exact", "keyword", "meaning", "margin", "hub", "recency", "fresh", "frecency", "prefix", "boost", "prefer",
    "app", "setting", "file", "image", "video", "audio", "clip", "emoji", "history", "photo", "generic",
];
/// Starting weights, fitted by tests/tune (tests/tuned.toml); picks then adjust them per user.
pub const DEFAULT_WEIGHTS: [f32; NF] = [
    1.0, 0.2, 0.3, 0.9, 0.25, 0.2, 0.05, 0.03, 0.08, 0.3, 1.0, 0.05,
    0.05, 0.03, 0.2, 0.05, 0.25, 0.0, -0.02, -0.03, -0.02, 0.0, 0.0,
];
const KINDS: [Kind; 9] =
    [Kind::App, Kind::Setting, Kind::File, Kind::Image, Kind::Video, Kind::Audio, Kind::Clip, Kind::Emoji, Kind::History];
const NAME: usize = 0;
const EXACT: usize = 1;
const KEYWORD: usize = 2;
const MEANING: usize = 3;
const MARGIN: usize = 4;
const HUB: usize = 5;
const RECENCY: usize = 6;
const FRESH: usize = 7;
const FRECENCY: usize = 8;
const PREFIX: usize = 9;
const BOOST: usize = 10;
const PREFER: usize = 11;
pub const PHOTO: usize = 21;
pub const GENERIC: usize = 22;
/// Cosine above the cutoff that counts as a full meaning match.
const MEANING_SPAN: f32 = 0.15;
const PIN_SCORE: f32 = 10.0;
/// Vector candidates looked at per query.
const CANDIDATES: usize = 300;
const LEARN_RATE: f32 = 0.05;
/// Keyword matches looked at per query, newest parts first.
const KEYWORD_SCAN: usize = 500;

/// Everything behind one row's score.
#[derive(Clone, Default)]
pub struct Sig {
    pub f: [f32; NF],
    pub score: f32,
    pub kept: bool,
    pub pinned: bool,
    pub excluded: bool,
    pub name: f32,
    pub bm25: f32,
    pub cosine: f32,
    pub cutoff: f32,
    pub hub: f32,
    /// Cosine of the best matching picture to the generic captions.
    pub generic: Option<f32>,
    hits: Vec<H>,
    /// Parts for the snippet, best first: the row shows the first with text
    /// (a document's overview part has none).
    snip: Vec<i64>,
}

#[derive(Clone)]
struct H {
    loc: Locator,
    score: f32,
    part: i64,
    thumb: Option<String>,
}

impl Sig {
    pub fn why(&self, w: &[f32; NF]) -> Why {
        let c = |i: usize| self.f[i] * w[i];
        let (name, kw, meaning) = (c(NAME) + c(EXACT), c(KEYWORD), c(MEANING) + c(MARGIN) + c(HUB) + c(PHOTO) + c(GENERIC));
        match () {
            _ if self.pinned => Why::Pinned,
            _ if name.max(kw).max(meaning) <= 0.0 => Why::Recent,
            _ if name >= kw && name >= meaning => Why::Name,
            _ if kw >= meaning => Why::Keyword,
            _ => Why::Meaning,
        }
    }
}

pub fn norm(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The query's words for keywords and highlights, without little words
/// ("connect to wifi" needs "connect" and "wifi"), unless that is all there is.
fn tokens(q: &str) -> Vec<String> {
    const LITTLE: &[&str] = &["a", "an", "and", "are", "at", "by", "do", "does", "for", "from", "how", "i", "in", "is", "it",
        "me", "my", "of", "on", "or", "the", "to", "what", "when", "where", "which", "with"];
    let all: Vec<String> = q.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(String::from).collect();
    let words: Vec<String> = all.iter().filter(|t| !LITTLE.contains(&t.as_str())).cloned().collect();
    if words.is_empty() { all } else { words }
}

pub fn cutoff(prefs: &crate::prefs::Prefs, name: &str) -> f32 {
    prefs.cutoffs.get(name).map(|v| *v as f32).unwrap_or_else(|| CUTOFFS.iter().find(|c| c.0 == name).map_or(0.64, |c| c.1))
}

/// Fuzzy name score per item: 1.0 is as good as the query matched against itself.
fn names(mem: &Mem, q: &str) -> HashMap<i64, (f32, bool)> {
    let mut out = HashMap::new();
    let q = q.trim();
    if q.is_empty() {
        return out;
    }
    let pat = Pattern::new(q, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
    let mut cfg = Config::DEFAULT;
    cfg.prefer_prefix = true;
    let mut m = Matcher::new(cfg);
    let mut buf = Vec::new();
    let best = pat.score(Utf32Str::new(q, &mut buf), &mut m).unwrap_or(1).max(1) as f32;
    let (ql, words, mut idx) = (q.to_lowercase(), q.split_whitespace().count(), Vec::new());
    for it in mem.items.values() {
        let (mut top, mut exact) = (0f32, false);
        // The title counts fully, keywords and aliases a bit less.
        for (s, weight) in std::iter::once((&it.title, 1.0)).chain(it.names.iter().map(|n| (n, 0.85))) {
            let Some(sc) = pat.score(Utf32Str::new(s, &mut buf), &mut m) else { continue };
            exact |= s.len() == ql.len() && s.to_lowercase() == ql;
            if weight * sc as f32 > top {
                // Letters scattered over several words ("wifi" in "With Fingers") count less.
                idx.clear();
                pat.indices(Utf32Str::new(s, &mut buf), &mut m, &mut idx);
                let runs = idx.windows(2).filter(|w| w[1] != w[0] + 1).count() + 1;
                top = top.max(weight * sc as f32 * 0.85f32.powi(runs.saturating_sub(words) as i32));
            }
        }
        if top > 0.0 {
            out.insert(it.id, ((top / best).min(1.0), exact));
        }
    }
    out
}

/// FTS5 expression: every word must match; the word being typed is a prefix.
fn fts_expr(q: &str) -> Option<String> {
    let toks = tokens(q);
    let typing = !q.ends_with(char::is_whitespace);
    let last = toks.len().checked_sub(1)?;
    let words: Vec<String> = toks
        .iter()
        .enumerate()
        .map(|(i, t)| if i == last && typing && t.chars().count() >= 2 { format!("\"{t}\"*") } else { format!("\"{t}\"") })
        .collect();
    Some(words.join(" "))
}

/// (part, bm25) best first; bm25 is positive here (FTS5 returns it negated).
/// bm25 reads every match of every word, too slow for the instant phase on a
/// Pi when a word is common, so `ranked` is off there (all scores 0).
fn keywords(c: &Connection, q: &str, ranked: bool) -> Vec<(i64, f32)> {
    let Some(expr) = fts_expr(q) else { return Vec::new() };
    let run = || -> rusqlite::Result<Vec<(i64, f32)>> {
        let mut st = c.prepare_cached(if ranked {
            "select rowid, -bm25(fts, 3.0, 1.0) from fts where fts match ?1 order by rowid desc limit ?2"
        } else {
            "select rowid, 0.0 from fts where fts match ?1 order by rowid desc limit ?2"
        })?;
        let rows = st.query_map(rusqlite::params![expr, KEYWORD_SCAN as i64], |r| Ok((r.get(0)?, r.get::<_, f64>(1)? as f32)))?;
        let mut v: Vec<(i64, f32)> = rows.collect::<rusqlite::Result<_>>()?;
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(CANDIDATES);
        Ok(v)
    };
    run().unwrap_or_default()
}

fn rule_hits(r: &crate::prefs::Rule, it: &Item, home: &Path) -> bool {
    let path_ok = |p: &String| {
        let want = p.strip_prefix("~/").map_or_else(|| Path::new(p).to_path_buf(), |rest| home.join(rest));
        it.key.strip_prefix("file:").is_some_and(|rel| home.join(rel).starts_with(&want))
    };
    (r.path.is_some() || r.kind.is_some() || r.key.is_some())
        && r.kind.is_none_or(|k| k == it.kind)
        && r.key.as_ref().is_none_or(|k| *k == it.key)
        && r.path.as_ref().is_none_or(path_ok)
}

fn pick_weights(picks: &[Pick], qn: &str, now: i64) -> (HashMap<String, f32>, HashMap<String, f32>) {
    let (mut prefix, mut item) = (HashMap::new(), HashMap::new());
    for p in picks {
        let decay = 0.5f32.powf((now - p.ts).max(0) as f32 / (14.0 * 86400.0));
        *item.entry(p.key.clone()).or_insert(0.0) += decay;
        if !qn.is_empty() && p.q.starts_with(qn) {
            *prefix.entry(p.key.clone()).or_insert(0.0) += decay * if p.q == qn { 1.0 } else { 0.6 };
        }
    }
    (prefix, item)
}

fn cos(mem: &Mem, row: usize, q: &QVec) -> f32 {
    dot(mem.vec(row), &q.0) as f32 * mem.scales[row] * q.1
}

/// Every candidate for `q` (plus `extra`), scored. Not sorted; check `kept`.
pub fn rank(d: &Daemon, mem: &Mem, q: &str, qv: Option<&QVecs>, extra: Option<i64>) -> Vec<(i64, Sig)> {
    let prefs = d.prefs.read().unwrap();
    let w = *d.weights.read().unwrap();
    let prefer = d.prefer.read().unwrap();
    let (qn, toks) = (norm(q), tokens(q));
    let now = now();
    let names = names(mem, q);
    let mut kw: HashMap<i64, Vec<(i64, f32)>> = HashMap::new();
    for (pid, s) in d.store.read(|c| Ok(keywords(c, q, qv.is_some()))).unwrap_or_default() {
        if let Some(p) = mem.parts.get(&pid) {
            kw.entry(p.item).or_default().push((pid, s));
        }
    }
    let scores: Vec<f32> = qv.map(|v| (0..mem.rows.len()).map(|r| cos(mem, r, &v.q)).collect()).unwrap_or_default();
    let (photo, generic) = (qv.and_then(|v| v.photo.as_ref()), d.generic.get().filter(|_| w[GENERIC] != 0.0));
    let (prefix, recent) = pick_weights(&d.picks.read().unwrap(), &qn, now);
    let pins: HashSet<&str> = prefs.pin.iter().filter(|p| norm(&p.q) == qn).map(|p| p.key.as_str()).collect();

    let mut ids: HashSet<i64> = names.keys().chain(kw.keys()).copied().chain(extra).collect();
    let mut top: Vec<usize> = (0..scores.len()).collect();
    if top.len() > CANDIDATES {
        top.select_nth_unstable_by(CANDIDATES, |a, b| scores[*b].total_cmp(&scores[*a]));
        top.truncate(CANDIDATES);
    }
    ids.extend(top.iter().filter_map(|&r| mem.parts.get(&mem.rows[r]).map(|p| p.item)));
    ids.extend(prefix.keys().map(String::as_str).chain(pins.iter().copied()).filter_map(|k| mem.keys.get(k)));

    let centroid = quantize(&normalize(mem.sum.clone()));
    let (ncut, fcut) = (cutoff(&prefs, "name"), cutoff(&prefs, "frame"));
    let mut out: Vec<(i64, Sig)> = Vec::new();
    for id in ids {
        let Some(it) = mem.items.get(&id) else { continue };
        let mut s = Sig { cutoff: cutoff(&prefs, &kind_name(it.kind)), ..Sig::default() };
        let f = &mut s.f;
        if let Some(&(n, exact)) = names.get(&id) {
            s.name = n;
            f[NAME] = if exact { 1.0 } else { ((n - ncut) / (1.0 - ncut)).clamp(0.0, 1.0) };
            f[EXACT] = exact as u8 as f32;
        }
        let kparts = kw.get(&id);
        if let Some(&(_, b)) = kparts.and_then(|v| v.first()) {
            s.bm25 = b;
            f[KEYWORD] = 0.3 + 0.7 * b.max(0.0) / (b.max(0.0) + 3.0);
        }
        // Meaning: each part against its own cutoff (pictures score higher for
        // everything); a picture also against "a photo of <q>", keeping the better.
        let (mut above, mut best, mut plain) = (HashMap::new(), None::<(i64, usize, f32)>, 0f32);
        for &pid in &it.parts {
            let Some((pm, r)) = mem.parts.get(&pid).and_then(|p| p.row.filter(|_| !scores.is_empty()).map(|r| (p, r))) else { continue };
            let picture = pm.image.is_some();
            let relevance = |c: f32| ((c - if picture { fcut } else { s.cutoff }) / MEANING_SPAN).clamp(0.0, 1.0);
            plain = plain.max(relevance(scores[r]));
            let c = match photo.filter(|_| picture) {
                Some(pv) => scores[r].max(cos(mem, r, pv)),
                None => scores[r],
            };
            let rel = relevance(c);
            s.cosine = s.cosine.max(c);
            if rel > 0.0 {
                above.insert(pid, c);
                if best.is_none_or(|b| rel > b.2) {
                    best = Some((pid, r, rel));
                }
            }
        }
        if let Some((pid, r, rel)) = best {
            f[MEANING] = plain;
            f[PHOTO] = rel - plain;
            if let Some(g) = generic.filter(|_| mem.parts[&pid].image.is_some()) {
                s.generic = Some(cos(mem, r, g));
            }
            s.hub = cos(mem, r, &centroid);
            f[HUB] = ((above[&pid] - s.hub) * 2.0).clamp(0.0, 1.0);
            if !prefer.is_empty() {
                let p = it.parts.iter().filter_map(|pid| mem.parts.get(pid)?.row).flat_map(|r| prefer.iter().map(move |v| (r, v)));
                let pc = p.map(|(r, v)| cos(mem, r, v)).fold(f32::MIN, f32::max);
                f[PREFER] = ((pc - s.cutoff) / MEANING_SPAN).clamp(0.0, 1.0);
            }
        }
        if it.mtime > 0 {
            f[RECENCY] = (-((now - it.mtime).max(0) as f32) / (30.0 * 86400.0)).exp();
        }
        f[FRESH] = (-((now - it.added).max(0) as f32) / (3.0 * 86400.0)).exp();
        let (pw, rw) = (prefix.get(&it.key).copied().unwrap_or(0.0), recent.get(&it.key).copied().unwrap_or(0.0));
        f[PREFIX] = 1.0 - (-pw).exp();
        f[FRECENCY] = 1.0 - (-rw / 3.0).exp();
        let by = |rules: &[crate::prefs::Rule]| rules.iter().filter(|r| rule_hits(r, it, &d.cfg.home)).map(|r| r.by.unwrap_or(0.1) as f32).sum::<f32>();
        f[BOOST] = by(&prefs.boost) - by(&prefs.bury);
        if let Some(k) = KINDS.iter().position(|k| *k == it.kind) {
            f[12 + k] = 1.0;
        }
        s.pinned = pins.contains(it.key.as_str());
        s.excluded = prefs.exclude.iter().any(|r| rule_hits(r, it, &d.cfg.home));
        s.kept = !s.excluded && (f[NAME] > 0.0 || f[KEYWORD] > 0.0 || f[MEANING] + f[PHOTO] > 0.0 || f[PREFIX] > 0.0 || s.pinned);
        s.hits = hits(mem, it, &above, kparts, &toks);
        let mut by_meaning: Vec<_> = above.iter().filter(|x| mem.parts.get(x.0).is_some_and(|p| p.image.is_none())).collect();
        by_meaning.sort_by(|a, b| b.1.total_cmp(a.1));
        let kw_snip = kparts.and_then(|v| v.first()).map(|p| p.0);
        s.snip = match s.why(&w) {
            Why::Meaning => by_meaning.iter().map(|x| *x.0).chain(kw_snip).collect(),
            _ => kw_snip.into_iter().collect(),
        };
        out.push((id, s));
    }
    // Generic: relative to the other pictures this query matched.
    let gens: Vec<f32> = out.iter().filter_map(|x| x.1.generic).collect();
    let mean = gens.iter().sum::<f32>() / gens.len().max(1) as f32;
    for (_, s) in &mut out {
        s.f[GENERIC] = s.generic.map_or(0.0, |g| g - mean);
    }
    // Margin: how far a meaning match leads the next one.
    let mut m: Vec<(usize, f32)> = out.iter().enumerate().filter(|x| x.1.1.f[MEANING] + x.1.1.f[PHOTO] > 0.0).map(|(i, x)| (i, x.1.cosine)).collect();
    m.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (j, &(i, c)) in m.iter().enumerate() {
        let next = m.get(j + 1).map_or(out[i].1.cutoff, |x| x.1);
        out[i].1.f[MARGIN] = ((c - next) * 5.0).clamp(0.0, 1.0);
    }
    for (_, s) in &mut out {
        s.score = s.f.iter().zip(&w).map(|(f, w)| f * w).sum::<f32>() + if s.pinned { PIN_SCORE } else { 0.0 };
    }
    out
}

/// A row's jump points: parts above the cutoff (neighbouring frames merged
/// into one moment), keyword-matched parts (pictures only when nothing matched
/// by meaning: their keywords are just the title) and sheets the query names.
/// Named parts first, then parts matching by keyword and meaning, by meaning,
/// by keyword. Only parts with somewhere to jump to (a locator or a picture).
fn hits(mem: &Mem, it: &Item, above: &HashMap<i64, f32>, kw: Option<&Vec<(i64, f32)>>, toks: &[String]) -> Vec<H> {
    // Photos and video frames show their small thumbnail; other pictures (image clips) themselves.
    let media = matches!(it.kind, Kind::Image | Kind::Video);
    let thumb = |img: &String| if media { ss_media::thumb(Path::new(img)).display().to_string() } else { img.clone() };
    let h = |pid: i64, score: f32| H { loc: mem.parts[&pid].locator.clone(), score, part: pid, thumb: mem.parts[&pid].image.as_ref().map(thumb) };
    let kw: HashMap<i64, f32> = kw.into_iter().flatten().copied().collect();
    let named = |pid: i64| mem.parts[&pid].locator.sheet.as_deref().map(tokens).is_some_and(|s| !s.is_empty() && s.iter().all(|t| toks.contains(t)));
    let (mut out, mut run) = (Vec::new(), Vec::new());
    let flush = |run: &mut Vec<i64>, out: &mut Vec<H>| {
        let Some(&best) = run.iter().max_by(|a, b| above[a].total_cmp(&above[b])) else { return };
        let (first, last) = (&mem.parts[&run[0]].locator, &mem.parts[run.last().unwrap()].locator);
        let loc = Locator { t: first.t, t_end: last.t_end.or(last.t).filter(|_| run.len() > 1).or(first.t_end), ..first.clone() };
        out.push(H { loc, ..h(best, above[&best]) });
        run.clear();
    };
    for &pid in &it.parts {
        let Some(pm) = mem.parts.get(&pid) else { continue };
        let b = kw.get(&pid).copied();
        match (pm.image.is_some(), above.get(&pid)) {
            (true, Some(_)) => run.push(pid),
            (true, None) => {
                flush(&mut run, &mut out);
                out.extend(b.filter(|_| above.is_empty()).map(|b| h(pid, b)));
            }
            (false, Some(&c)) => out.push(h(pid, c)),
            (false, None) => out.extend(b.or(named(pid).then_some(0.0)).map(|b| h(pid, b))),
        }
    }
    flush(&mut run, &mut out);
    out.retain(|x| x.thumb.is_some() || x.loc != Locator::default());
    let tier = |x: &H| {
        let (n, m) = (named(x.part), above.contains_key(&x.part));
        (n, (n || kw.contains_key(&x.part)) && m, m)
    };
    out.sort_by(|a, b| tier(b).cmp(&tier(a)).then(b.score.total_cmp(&a.score)));
    out.truncate(8);
    out
}

/// Top hit first; after it, rows within the same 0.1 band of score group by
/// kind (kinds in order of their best row).
fn order(mem: &Mem, v: &mut [(i64, Sig)]) {
    v.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(a.0.cmp(&b.0)));
    let Some(rest) = v.get_mut(1..) else { return };
    let mut first: HashMap<Kind, usize> = HashMap::new();
    for (id, _) in rest.iter() {
        let n = first.len();
        first.entry(mem.items[id].kind).or_insert(n);
    }
    let key = |x: &(i64, Sig)| (-(x.1.score * 10.0).floor() as i32, first[&mem.items[&x.0].kind]);
    rest.sort_by(|a, b| key(a).cmp(&key(b)).then(b.1.score.total_cmp(&a.1.score)).then(a.0.cmp(&b.0)));
}

fn clip(text: &str, n: usize) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n { t } else { t.chars().take(n).collect::<String>() + "…" }
}

/// About 160 characters around the first query word, with every query word
/// (at a word start) marked. Marks are character offsets, end exclusive.
pub fn snippet(text: &str, toks: &[String]) -> Snippet {
    let chars: Vec<char> = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let toks: Vec<Vec<char>> = toks.iter().map(|t| t.chars().collect()).collect();
    let mut marks = Vec::new();
    let mut i = 0;
    while i < lower.len() {
        let at_word = i == 0 || !lower[i - 1].is_alphanumeric();
        match toks.iter().filter(|t| at_word && lower[i..].starts_with(t)).map(Vec::len).max() {
            Some(n) => {
                marks.push((i, i + n));
                i += n;
            }
            None => i += 1,
        }
    }
    let start = match marks.first() {
        Some(&(m, _)) if m > 30 => (m - 30..m).find(|&j| chars[j - 1] == ' ').unwrap_or(m),
        _ => 0,
    };
    let end = (start + 160).min(chars.len());
    Snippet {
        text: chars[start..end].iter().collect(),
        marks: marks.into_iter().filter(|m| m.0 >= start && m.1 <= end).map(|(a, b)| (a - start, b - start)).collect(),
    }
}

fn make_row(c: &Connection, it: &Item, s: &Sig, toks: &[String], w: &[f32; NF]) -> Row {
    let hits: Vec<Hit> = s
        .hits
        .iter()
        .map(|h| Hit { locator: h.loc.clone(), score: h.score, text: clip(&part_text(c, h.part), 140), thumb: h.thumb.clone() })
        .collect();
    let snippet = s.snip.iter().map(|&p| snippet(&part_text(c, p), toks)).find(|x| !x.text.is_empty());
    let mut action = it.action.clone();
    if let (Action::Open { locator, .. }, Some(h)) = (&mut action, hits.first()) {
        *locator = h.locator.clone();
    }
    Row { key: it.key.clone(), kind: it.kind, title: it.title.clone(), subtitle: it.subtitle.clone(), icon: it.icon.clone(), action,
        score: (s.score * 1000.0).round() / 1000.0, why: s.why(w), snippet, hits }
}

fn rows(d: &Daemon, mem: &Mem, ranked: &[(i64, Sig)], q: &str) -> Vec<Row> {
    let (toks, w) = (tokens(q), *d.weights.read().unwrap());
    let build = |c: &Connection| Ok(ranked.iter().map(|(id, s)| make_row(c, &mem.items[id], s, &toks, &w)).collect());
    d.store.read(build).unwrap_or_default()
}

pub fn web_row(q: &str) -> Row {
    let q = q.trim();
    let enc: String = q
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    Row { key: format!("web:{q}"), kind: Kind::Web, title: format!("Search the web for “{q}”"), subtitle: "DuckDuckGo".into(),
        icon: "web-browser".into(), action: Action::Url { url: format!("https://duckduckgo.com/?q={enc}") }, score: 0.0,
        why: Why::Keyword, snippet: None, hits: Vec::new() }
}

/// Ranked rows (web row last) and whether nothing local matched; only rows
/// of `kinds` when given. An empty query lists recently picked items instead.
pub fn search(d: &Daemon, q: &str, limit: usize, qv: Option<&QVecs>, kinds: Option<&[Kind]>) -> (Vec<Row>, bool) {
    let mem = d.store.mem.read().unwrap();
    let wanted = |id: &i64| kinds.is_none_or(|k| k.contains(&mem.items[id].kind));
    let web = !norm(q).is_empty() && kinds.is_none_or(|k| k.contains(&Kind::Web));
    let mut ranked: Vec<(i64, Sig)> = if norm(q).is_empty() {
        let (_, recent) = pick_weights(&d.picks.read().unwrap(), "", now());
        let mut v: Vec<(i64, Sig)> =
            recent.iter().filter_map(|(k, w)| Some((*mem.keys.get(k)?, Sig { score: *w, ..Sig::default() }))).collect();
        v.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
        v
    } else {
        let mut v: Vec<(i64, Sig)> = rank(d, &mem, q, qv, None).into_iter().filter(|x| x.1.kept).collect();
        order(&mem, &mut v);
        v
    };
    ranked.retain(|x| wanted(&x.0));
    ranked.truncate(limit.max(1) - usize::from(web));
    let mut rows = rows(d, &mem, &ranked, q);
    let empty = rows.is_empty();
    if web {
        let mut row = web_row(q);
        // The default browser's own icon, from its indexed desktop entry.
        if let Some(id) = d.browser.read().unwrap().as_ref().and_then(|b| mem.keys.get(&format!("app:{b}"))) {
            row.icon = mem.items[id].icon.clone();
        }
        rows.push(row);
    }
    (rows, empty)
}

/// The row for `key` as `q` ranks it (for activation), after learning from the
/// pick: the picked row should beat every row that was shown above it.
pub fn pick(d: &Daemon, q: &str, key: &str, qv: Option<&QVecs>) -> Result<Row> {
    let mem = d.store.mem.read().unwrap();
    let id = *mem.keys.get(key).with_context(|| format!("{key} is not indexed"))?;
    let mut ranked = rank(d, &mem, q, qv, Some(id));
    ranked.retain(|x| x.1.kept || x.0 == id);
    order(&mem, &mut ranked);
    let pos = ranked.iter().position(|x| x.0 == id).unwrap();
    if !norm(q).is_empty() && ranked[pos].1.kept && pos > 0 {
        let mut w = d.weights.write().unwrap();
        let fp = ranked[pos].1.f;
        for (_, s) in ranked[..pos].iter().filter(|x| !x.1.pinned) {
            let diff: Vec<f32> = fp.iter().zip(&s.f).map(|(a, b)| a - b).collect();
            if diff.iter().zip(w.iter()).map(|(x, w)| x * w).sum::<f32>() < 0.05 {
                for (i, x) in diff.iter().enumerate().filter(|(i, _)| *i != BOOST) {
                    w[i] = (w[i] + LEARN_RATE * x).clamp(-2.0, 3.0);
                }
            }
        }
        save_weights(&d.cfg.state.join("weights.json"), &w);
    }
    rows(d, &mem, &ranked[pos..=pos], q).pop().context("index unreadable")
}

pub fn load_weights(path: &Path) -> [f32; NF] {
    let saved: HashMap<String, f32> = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    std::array::from_fn(|i| saved.get(FEATURES[i]).copied().unwrap_or(DEFAULT_WEIGHTS[i]))
}

fn save_weights(path: &Path, w: &[f32; NF]) {
    let m: serde_json::Map<String, Value> = FEATURES.iter().zip(w).map(|(n, w)| (n.to_string(), json!(w))).collect();
    let tmp = path.with_extension("json.new");
    if std::fs::write(&tmp, Value::Object(m).to_string()).and_then(|_| std::fs::rename(&tmp, path)).is_err() {
        eprintln!("superspace: could not save {}", path.display());
    }
}

/// Every signal behind `key`'s score for `q`, each feature with its weight and contribution.
pub fn explain(d: &Daemon, q: &str, key: &str) -> Result<Value> {
    let qv = d.query_vec(q);
    let (shown, empty) = search(d, q, 200, qv.as_ref(), None);
    let mem = d.store.mem.read().unwrap();
    let id = *mem.keys.get(key).with_context(|| format!("{key} is not indexed"))?;
    let (_, s) = rank(d, &mem, q, qv.as_ref(), Some(id)).into_iter().find(|x| x.0 == id).unwrap();
    let w = *d.weights.read().unwrap();
    let features: serde_json::Map<String, Value> = FEATURES
        .iter()
        .enumerate()
        .filter(|(i, _)| s.f[*i] != 0.0)
        .map(|(i, n)| (n.to_string(), json!({"value": s.f[i], "weight": w[i], "contribution": s.f[i] * w[i]})))
        .collect();
    let it = &mem.items[&id];
    let best = d.store.read(|c| Ok(s.snip.iter().map(|&p| clip(&part_text(c, p), 300)).find(|t| !t.is_empty()))).unwrap_or_default();
    Ok(json!({
        "key": key, "title": it.title, "kind": it.kind, "q": q,
        "rank": shown.iter().position(|r| r.key == key), "shown": shown.len(), "empty": empty,
        "kept": s.kept, "why": s.why(&w), "score": s.score, "meaning_used": qv.is_some(),
        "raw": {"name": s.name, "bm25": s.bm25, "cosine": s.cosine, "cutoff": s.cutoff, "hub": s.hub, "generic": s.generic,
                "pinned": s.pinned, "excluded": s.excluded, "parts": it.parts.len(),
                "embedded_parts": it.parts.iter().filter(|p| mem.parts.get(p).is_some_and(|p| p.row.is_some())).count()},
        "features": features, "best_part": best,
        "rule": "kept when name, keyword or meaning (above the cutoff) matches, or a pin or a pick for this prefix; never when excluded",
    }))
}

/// Items closest in meaning to `key` (its parts averaged).
pub fn similar(d: &Daemon, key: &str, limit: usize) -> Result<Vec<Row>> {
    let mem = d.store.mem.read().unwrap();
    let id = *mem.keys.get(key).with_context(|| format!("{key} is not indexed"))?;
    let mut acc = vec![0f32; DIM];
    for r in mem.items[&id].parts.iter().filter_map(|p| mem.parts.get(p)?.row) {
        acc.iter_mut().zip(mem.vec(r)).for_each(|(a, &x)| *a += x as f32 * mem.scales[r]);
    }
    anyhow::ensure!(acc.iter().any(|x| *x != 0.0), "{key} has no vectors yet");
    let qv = QVecs { q: quantize(&normalize(acc)), photo: None };
    let mut ranked: Vec<_> = rank(d, &mem, "", Some(&qv), None).into_iter().filter(|x| x.0 != id && x.1.kept).collect();
    ranked.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
    ranked.truncate(limit.max(1));
    Ok(rows(d, &mem, &ranked, ""))
}

/// An item and the text of its parts (all, or part `part`, 0-based), within ~24k characters.
pub fn read(d: &Daemon, key: &str, part: Option<usize>) -> Result<Value> {
    let mem = d.store.mem.read().unwrap();
    let it = mem.keys.get(key).and_then(|id| mem.items.get(id)).with_context(|| format!("{key} is not indexed"))?;
    let mut budget = 24_000usize;
    let mut parts = Vec::new();
    let mut truncated = false;
    d.store.read(|c| {
        for (n, pid) in it.parts.iter().enumerate().filter(|(n, _)| part.is_none_or(|p| p == *n)) {
            if budget == 0 {
                truncated = true;
                break;
            }
            let text = part_text(c, *pid);
            let cut: String = text.chars().take(budget).collect();
            budget = budget.saturating_sub(cut.chars().count());
            truncated |= cut.len() < text.len();
            parts.push(json!({"n": n, "locator": mem.parts[pid].locator, "text": cut}));
        }
        Ok(())
    })?;
    Ok(json!({"key": it.key, "kind": it.kind, "title": it.title, "subtitle": it.subtitle, "action": it.action,
        "total_parts": it.parts.len(), "truncated": truncated, "parts": parts}))
}
