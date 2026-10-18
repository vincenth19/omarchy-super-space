//! Types shared by every Super Space crate: what sources hand to the index
//! (`Doc`, `Part`), what search hands to clients (`Row`, `Answer`), and the
//! socket protocol (`Request`, `Response`). See DESIGN.md.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    App,
    Setting,
    File,
    Image,
    Video,
    Audio,
    Clip,
    Emoji,
    History,
    Web,
}

/// Where inside an item a part lives. All numbers are 1-based; times are seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Locator {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t_end: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// `uwsm-app -- gtk-launch <desktop_id>`
    Launch { desktop_id: String },
    /// Open a file, at the locator when the viewer supports it (PDF page, media time).
    Open { path: String, #[serde(default)] locator: Locator },
    Run { argv: Vec<String> },
    Copy { text: String },
    /// Copy, then paste into the window that had focus.
    Paste { text: String },
    Url { url: String },
}

/// One searchable thing, as a source produces it.
#[derive(Clone, Debug)]
pub struct Doc {
    /// Stable id: "app:<desktop id>", "file:<path relative to $HOME>", "setting:<id>",
    /// "clip:<sha1 of text>", "emoji:<emoji>", "history:<url>".
    pub key: String,
    pub kind: Kind,
    pub title: String,
    /// Shown under the title, e.g. "~/Documents".
    pub subtitle: String,
    /// A freedesktop icon name, a desktop id, or an absolute image path.
    pub icon: String,
    pub action: Action,
    /// Changes whenever the content changes; an unchanged stamp is never re-indexed.
    pub stamp: String,
    /// Extra strings matched instantly by name (keywords, aliases, generic names).
    pub names: Vec<String>,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug)]
pub struct Part {
    /// Snippet text, also indexed for keyword search. May be empty (a video frame).
    pub text: String,
    pub locator: Locator,
    pub embed: Embed,
}

#[derive(Clone, Debug)]
pub enum Embed {
    /// Keywords only.
    None,
    /// The exact model input; build documents with `doc_text`.
    Text(String),
    /// A JPEG or PNG (photo, video frame, thumbnail); embedded with no prefix.
    Image(PathBuf),
}

/// EmbeddingGemma 2's document format.
pub fn doc_text(title: &str, body: &str) -> String {
    format!("title: {} | text: {}", if title.is_empty() { "none" } else { title }, body)
}

/// EmbeddingGemma 2's search-query format.
pub fn query_text(q: &str) -> String {
    format!("task: search result | query: {q}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    Name,
    Keyword,
    Meaning,
    Pinned,
    Recent,
}

/// A part of a result worth jumping to: a page, a slide, a moment in a video.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub locator: Locator,
    pub score: f32,
    pub text: String,
    /// Absolute path of a small JPEG, for video moments and photos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snippet {
    pub text: String,
    /// Character ranges (start, end) of `text` to highlight.
    pub marks: Vec<(usize, usize)>,
}

/// One search result row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    pub action: Action,
    pub score: f32,
    pub why: Why,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<Snippet>,
    /// Best parts first, at most 8.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<Hit>,
}

/// An instant answer, drawn as a two-sided card: input → output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    /// "math", "units", "currency", "time", "percent", "dictionary".
    pub kind: String,
    pub input: String,
    pub input_label: String,
    pub output: String,
    pub output_label: String,
    /// What Enter copies.
    pub copy: String,
    /// One small line under the card, e.g. "ECB rate of 16 Oct".
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// `kinds`: only rows of these kinds (the web row only when it lists `web`).
    Search { id: u64, q: String, #[serde(default = "default_limit")] limit: usize, #[serde(default)] kinds: Option<Vec<Kind>> },
    /// Run a row's action; `hit` picks a part (page, moment) to open at; `alt` is the
    /// alternative action (show a file in its folder, else copy what the row points at).
    Activate { id: u64, key: String, #[serde(default)] q: String, #[serde(default)] hit: Option<usize>, #[serde(default)] alt: bool },
    Read { id: u64, key: String, #[serde(default)] part: Option<usize> },
    Similar { id: u64, key: String, #[serde(default = "default_limit")] limit: usize },
    Explain { id: u64, q: String, key: String },
    Prefs { id: u64 },
    /// `change` is one preferences edit, e.g. {"pin": {"q": "lease", "key": "file:…"}}.
    Pref { id: u64, change: serde_json::Value },
    Status { id: u64 },
}

fn default_limit() -> usize {
    30
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Names, keywords and answers: sent within a few milliseconds.
    Instant,
    /// Everything, ranked with meaning: sent once the query is embedded.
    Final,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Timing {
    pub instant_ms: f32,
    pub embed_ms: f32,
    pub total_ms: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Search {
        id: u64,
        phase: Phase,
        rows: Vec<Row>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answer: Option<Answer>,
        /// True when nothing local is a real match (only the web row is left).
        empty: bool,
        timing: Timing,
    },
    Value { id: u64, value: serde_json::Value },
    Error { id: u64, error: String },
}
