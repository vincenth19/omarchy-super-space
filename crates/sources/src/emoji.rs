//! Emoji with English and Indonesian names and keywords, embedded at build
//! time (data/emoji.tsv, made by data/make-emoji.py; Unicode licence, see data/NOTICE).
use crate::text_part;
use ss_core::{Action, Doc, Kind};

const DATA: &str = include_str!("../data/emoji.tsv");

pub fn emoji() -> Vec<Doc> {
    DATA.lines().filter_map(doc).collect()
}

/// One line: emoji, English name, English keywords, Indonesian name, Indonesian keywords.
fn doc(line: &str) -> Option<Doc> {
    let mut f = line.split('\t');
    let (e, name, en, id_name, id) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
    let words = |s: &str| s.split('|').filter(|w| !w.is_empty()).map(String::from).collect::<Vec<_>>();
    let mut names = words(en);
    let subtitle = names.iter().take(5).cloned().collect::<Vec<_>>().join(" · ");
    if !id_name.is_empty() {
        names.push(id_name.into());
    }
    names.extend(words(id));
    let mut chars = name.chars();
    let title: String = chars.next()?.to_uppercase().chain(chars).collect();
    let (stamp, parts) = text_part(&title, names.join(", "));
    Some(Doc { key: format!("emoji:{e}"), kind: Kind::Emoji, title, subtitle, icon: e.into(), action: Action::Paste { text: e.into() }, stamp, names, parts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_parses_and_keys_are_unique() {
        let docs = emoji();
        assert_eq!(docs.len(), DATA.lines().count());
        assert!(docs.len() > 1900, "{}", docs.len());
        let keys: std::collections::HashSet<_> = docs.iter().map(|d| &d.key).collect();
        assert_eq!(keys.len(), docs.len());
    }

    #[test]
    fn grimacing_is_awkward_in_both_languages() {
        let d = emoji().into_iter().find(|d| d.key == "emoji:😬").unwrap();
        assert_eq!(d.title, "Grimacing face");
        assert!(d.names.iter().any(|n| n == "awkward"));
        assert!(d.names.iter().any(|n| n == "nyengir"));
        assert_eq!(d.action, Action::Paste { text: "😬".into() });
        let dog = emoji().into_iter().find(|d| d.key == "emoji:🐕").unwrap();
        assert!(dog.names.iter().any(|n| n == "anjing"));
        let ss_core::Embed::Text(t) = &dog.parts[0].embed else { panic!() };
        assert_eq!(t, "title: Dog | text: animal, animals, dogs, pet, anjing, binatang, hewan, peliharaan");
    }

    #[test]
    fn no_skin_tone_variants() {
        assert!(!DATA.contains('\u{1F3FD}'));
        assert!(DATA.contains("👍\t"));
    }
}
