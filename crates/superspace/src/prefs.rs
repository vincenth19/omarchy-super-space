//! ~/.config/superspace/preferences.toml: pins, boosts, burials, exclusions,
//! `prefer` sentences and meaning-cutoff overrides. People and agents edit the
//! same file; `Pref` requests apply one change and rewrite it.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ss_core::Kind;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefer: Vec<String>,
    /// Meaning cutoffs by kind name (and "frame", "name"), overriding search::CUTOFFS.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub cutoffs: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pin: Vec<Pin>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub boost: Vec<Rule>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bury: Vec<Rule>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<Rule>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub q: String,
    pub key: String,
}

/// Matches items by any of path (a folder or file, `~` allowed), kind and key; all given must match.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<f64>,
}

pub fn load(path: &Path) -> Result<Prefs> {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).with_context(|| format!("{}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Prefs::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save(path: &Path, p: &Prefs) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, toml::to_string(p)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// One edit, e.g. {"pin": {"q": "lease", "key": "file:…"}} or {"prefer": "…"}.
/// "un" + a list name removes every entry that has all the given fields.
pub fn apply(p: &Prefs, change: &Value) -> Result<Prefs> {
    let Some((op, val)) = change.as_object().filter(|o| o.len() == 1).and_then(|o| o.iter().next()) else {
        bail!("a change is one {{\"op\": value}}, e.g. {{\"pin\": {{\"q\": \"lease\", \"key\": \"file:Documents/lease.pdf\"}}}}");
    };
    let (list, add) = op.strip_prefix("un").map_or((op.as_str(), true), |l| (l, false));
    if !["pin", "boost", "bury", "exclude", "prefer"].contains(&list) {
        bail!("unknown preference {op:?}: use pin, boost, bury, exclude, prefer or un… of them");
    }
    let mut v = serde_json::to_value(p)?;
    let arr = v.as_object_mut().unwrap().entry(list).or_insert(json!([])).as_array_mut().unwrap();
    let has = |x: &Value| match (val.as_object(), x.as_object()) {
        (Some(want), Some(got)) => want.iter().all(|(k, w)| got.get(k) == Some(w)),
        _ => x == val,
    };
    if add {
        if !arr.iter().any(|x| x == val) {
            arr.push(val.clone());
        }
    } else {
        let before = arr.len();
        arr.retain(|x| !has(x));
        if arr.len() == before {
            bail!("no {list} entry matches {val}");
        }
    }
    serde_json::from_value(v).context("bad preference value")
}
