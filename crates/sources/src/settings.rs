//! Omarchy's menu, its bar panels and its commands, as rows that run them.
use crate::{files_under, omarchy, read_json, text_part, xdg};
use serde::Deserialize;
use serde_json::{Map, Value};
use ss_core::{Action, Doc, Kind};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Nerd Font cog, for rows with no glyph of their own (menu rows carry Omarchy's).
const GEAR: &str = "\u{f013}";

/// Commands outside the menu are offered only when their summary starts with
/// one of these: things a person runs, not plumbing that prints for scripts.
const VERBS: &[&str] = &["Toggle", "Launch", "Open", "Restart", "Switch", "Cycle", "Lock", "Select and set", "Show the current"];

/// How long all the menu's `when:` checks together may take. All 152 take
/// 1.1-1.4 s on the Pi; a check cut off hides its row until the next read.
const CHECKS: Duration = Duration::from_secs(4);

/// Everyday words for rows people look for by what they want rather than by
/// Omarchy's label, by row id. Each names something the row really does
/// (Omarchy's summaries, panel descriptions and input.lua comments).
const PHRASES: &[(&str, &str)] = &[
    ("style.theme", "change the theme, colour scheme, color scheme, dark mode, light mode"),
    ("style.background", "change the wallpaper, desktop background, desktop picture"),
    ("omarchy-theme-bg-next", "next wallpaper"),
    ("trigger.toggle.nightlight", "night light, blue light filter, night mode, night shift, make the screen warmer"),
    ("omarchy.network", "wifi, wi-fi, wireless network, connect to wifi, internet connection"),
    ("update.hardware.wifi", "restart wifi, wifi not working"),
    ("omarchy.bluetooth", "pair bluetooth device, connect bluetooth headphones"),
    ("update.hardware.bluetooth", "restart bluetooth, bluetooth not working"),
    ("omarchy.audio", "volume, turn the volume up or down, sound, speakers, mute the sound, sound output"),
    ("omarchy-audio-input-mute", "mute the microphone, mic mute"),
    ("omarchy.monitor", "brightness, screen brightness, dim the screen"),
    ("trigger.capture.screenshot", "take a screenshot, screen capture, screen grab, print screen"),
    ("trigger.capture.screenrecord", "record the screen, screen recording, screencast"),
    ("system.lock", "lock the screen, lock screen, lock the computer"),
    ("system.shutdown", "shut down, power off, turn off the computer"),
    ("system.reboot", "restart the computer"),
    ("system.suspend", "sleep"),
    ("system.logout", "log out, sign out"),
    ("system", "power menu, power options"),
    ("setup.input", "keyboard layout, keyboard language, key repeat, mouse sensitivity, natural scrolling, touchpad"),
    ("trigger.emoji", "emoji picker, insert an emoji"),
    ("omarchy-menu-clipboard", "clipboard history, paste history"),
];

#[derive(Deserialize, Default)]
#[serde(default)]
struct Item {
    label: String,
    icon: String,
    action: String,
    target: String,
    provider: String,
    description: String,
    when: String,
    aliases: Value,
}

#[derive(Deserialize)]
struct Cmd {
    route: String,
    binary: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    args: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    requires_sudo: bool,
}

/// The default menu, the user's menu extension, and shell.json (which panels are in the bar).
pub(crate) fn files() -> Vec<PathBuf> {
    let config = xdg("XDG_CONFIG_HOME", ".config").join("omarchy");
    vec![omarchy().join("default/omarchy/omarchy-menu.jsonc"), config.join("extensions/omarchy-menu.jsonc"), config.join("shell.json")]
}

pub fn settings() -> Vec<Doc> {
    let paths = files();
    let texts: Vec<String> = paths[..2].iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    let items = menu_items(&texts);
    let conds: Vec<(&str, &str)> = items.iter().filter(|(_, i)| !i.when.is_empty()).map(|(id, i)| (id.as_str(), i.when.as_str())).collect();
    let ok = passing(&conds.iter().map(|c| c.1).collect::<Vec<_>>(), CHECKS);
    let failed: HashSet<&str> = conds.iter().enumerate().filter(|(n, _)| !ok.contains(n)).map(|(_, c)| c.0).collect();
    let cmds = commands();
    let summaries: HashMap<&str, &str> = cmds.iter().map(|c| (c.binary.as_str(), c.summary.as_str())).collect();
    let in_menu: HashSet<&str> = items.values().flat_map(|i| bins(&i.action)).collect();
    let mut docs = menu_docs(&items, &failed, &summaries);
    docs.extend(panel_docs(&read_json(&paths[2]), &manifests()));
    docs.extend(command_docs(&cmds, &in_menu));
    docs.iter_mut().for_each(add_phrases);
    docs
}

/// A row's PHRASES become names (instant matches) and part of its embedded text.
fn add_phrases(d: &mut Doc) {
    let Some(&(_, said)) = PHRASES.iter().find(|(id, _)| d.key.strip_prefix("setting:") == Some(*id)) else { return };
    d.names.extend(said.split(", ").map(String::from));
    let body = format!("{}. {said}", d.parts[0].text);
    (d.stamp, d.parts) = text_part(&d.title, body);
}

/// Menu items by id, the user's fields laid over the default ones, as Omarchy merges them.
fn menu_items(texts: &[String]) -> BTreeMap<String, Item> {
    let mut merged: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for text in texts {
        let Ok(Value::Object(mut top)) = serde_json::from_str(&strip_jsonc(text)) else { continue };
        if let Some(Value::Object(items)) = top.remove("items") {
            top = items;
        }
        for (id, v) in top {
            if let Value::Object(fields) = v {
                merged.entry(id).or_default().extend(fields);
            }
        }
    }
    merged.into_iter().filter_map(|(id, f)| Some((id, serde_json::from_value(Value::Object(f)).ok()?))).collect()
}

/// Omarchy's JSONC: whole-line // comments and trailing commas.
fn strip_jsonc(raw: &str) -> String {
    let text = raw.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    text.char_indices()
        .filter(|&(i, c)| !(c == ',' && matches!(text[i + 1..].trim_start().chars().next(), Some('}' | ']'))))
        .map(|(_, c)| c)
        .collect()
}

/// Indexes of the shell conditions that succeed, checked in parallel. A check
/// still running after `limit` counts as failed, so one that hangs cannot
/// hold up settings().
fn passing(conds: &[&str], limit: Duration) -> HashSet<usize> {
    let script: String = conds.iter().enumerate().map(|(i, c)| format!("(\n{c}\n) >/dev/null 2>&1 </dev/null && echo {i} &\n")).collect();
    let mut ok = HashSet::new();
    let Ok(mut child) = sh(&(script + "wait\n")).stdout(Stdio::piped()).process_group(0).spawn() else { return ok };
    let (tx, rx) = mpsc::channel();
    let out = BufReader::new(child.stdout.take().expect("piped"));
    std::thread::spawn(move || out.lines().map_while(Result::ok).try_for_each(|l| tx.send(l)));
    let end = Instant::now() + limit;
    while let Ok(line) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
        ok.extend(line.trim().parse::<usize>());
    }
    // bash and every check it started share one process group; bash is not
    // reaped yet, so the group id cannot belong to anything else.
    let _ = sh(&format!("kill -KILL -- -{}", child.id())).status();
    let _ = child.wait();
    ok
}

fn sh(script: &str) -> Command {
    let path = format!("{}:{}", omarchy().join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let mut c = Command::new("bash");
    c.args(["-c", script]).env("PATH", path).env("OMARCHY_PATH", omarchy()).stdin(Stdio::null()).stderr(Stdio::null());
    c
}

fn commands() -> Vec<Cmd> {
    #[derive(Deserialize)]
    struct List {
        commands: Vec<Cmd>,
    }
    let out = sh("omarchy commands --json").output().map(|o| o.stdout).unwrap_or_default();
    serde_json::from_slice::<List>(&out).map(|l| l.commands).unwrap_or_default()
}

/// The omarchy-* commands a shell line calls.
fn bins(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).filter(|t| t.starts_with("omarchy-"))
}

/// "a", "a.b", "a.b.c" for "a.b.c".
fn lineage(id: &str) -> impl Iterator<Item = &str> {
    id.match_indices('.').map(move |(i, _)| &id[..i]).chain([id])
}

fn run(argv: &[&str]) -> Action {
    Action::Run { argv: argv.iter().map(|s| s.to_string()).collect() }
}

fn menu_docs(items: &BTreeMap<String, Item>, failed: &HashSet<&str>, summaries: &HashMap<&str, &str>) -> Vec<Doc> {
    let label = |id: &str| items.get(id).map_or(id, |i| if i.label.is_empty() { id } else { i.label.as_str() }).to_string();
    let leaf = |i: &Item| !(i.action.is_empty() && i.target.is_empty() && i.provider.is_empty());
    // A row whose `when:` fails is hidden with everything under it, and a
    // submenu with nothing left to run is hidden too.
    let shown: Vec<(&str, &Item)> = items.iter().map(|(id, i)| (id.as_str(), i)).filter(|(id, _)| !lineage(id).any(|a| failed.contains(a))).collect();
    let under = |id: &str, o: &str| o.strip_prefix(id).is_some_and(|r| r.starts_with('.'));
    let kept: Vec<(&str, &Item)> = shown.iter().copied().filter(|&(id, i)| leaf(i) || shown.iter().any(|&(o, oi)| under(id, o) && leaf(oi))).collect();
    let mut docs = Vec::new();
    for &(id, item) in &kept {
        let trail: Vec<String> = lineage(id).filter(|a| *a != id).map(label).collect();
        let kids: Vec<String> = kept.iter().filter(|(o, _)| under(id, o) && !o[id.len() + 1..].contains('.')).map(|(o, _)| label(o)).take(12).collect();
        // What the row does, in Omarchy's own summaries of the commands it runs.
        let mut said = HashSet::new();
        let summary = bins(&item.action)
            .filter(|b| *b != "omarchy-launch-floating-terminal-with-presentation")
            .filter_map(|b| summaries.get(b).copied().filter(|s| said.insert(*s)))
            .collect::<Vec<_>>()
            .join(". ");
        let title = label(id);
        let path = trail.iter().chain([&title]).cloned().collect::<Vec<_>>().join(" › ");
        let includes = if kids.is_empty() { String::new() } else { format!("Includes {}", kids.join(", ")) };
        let body = [path, item.description.clone(), summary, includes].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(". ");
        let mut names: Vec<String> = match &item.aliases {
            Value::String(a) => vec![a.clone()],
            Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        };
        names.extend(trail.last().map(|p| format!("{p} {title}")));
        let action = match (item.action.as_str(), item.target.as_str()) {
            ("", "") => run(&["omarchy-menu", "summon", id]),
            ("", target) => run(&["omarchy-menu", "summon", target]),
            (line, _) => run(&["bash", "-lc", line]),
        };
        let (stamp, parts) = text_part(&title, body);
        docs.push(Doc {
            key: format!("setting:{id}"),
            kind: Kind::Setting,
            subtitle: if trail.is_empty() { "Omarchy menu".into() } else { trail.join(" › ") },
            icon: if item.icon.is_empty() { GEAR.into() } else { item.icon.clone() },
            title,
            action,
            stamp,
            names,
            parts,
        });
    }
    docs
}

fn manifests() -> Vec<Value> {
    let user = xdg("XDG_CONFIG_HOME", ".config").join("omarchy/plugins");
    [(omarchy().join("shell/plugins"), 3), (user, 2)]
        .iter()
        .flat_map(|(dir, depth)| files_under(dir, *depth, &|n| n == "manifest.json"))
        .map(|p| read_json(&p))
        .collect()
}

/// Bar panels (Wi-Fi, Bluetooth, audio…) that are in the bar, so summoning them opens them.
fn panel_docs(shell: &Value, manifests: &[Value]) -> Vec<Doc> {
    let layout = &shell["bar"]["layout"];
    let in_bar: HashSet<&str> = ["left", "center", "right"].iter().flat_map(|s| layout[s].as_array().into_iter().flatten()).filter_map(|w| w["id"].as_str()).collect();
    let mut docs = Vec::new();
    for m in manifests {
        let id = m["id"].as_str().unwrap_or("");
        let widget = m["kinds"].as_array().is_some_and(|k| k.iter().any(|k| k == "bar-widget"));
        if !widget || !in_bar.contains(id) || id == "omarchy.menu" {
            continue;
        }
        let title = m["barWidget"]["displayName"].as_str().or(m["name"].as_str()).unwrap_or(id).to_string();
        let about = m["description"].as_str().unwrap_or("").trim_end_matches('.').to_string();
        // Measured on the Pi: "Opens the … panel" lifts "turn up the volume" to Audio by 0.06.
        let (stamp, parts) = text_part(&title, format!("{about}. Opens the {title} panel").trim_start_matches(". ").into());
        docs.push(Doc { key: format!("setting:{id}"), kind: Kind::Setting, title, subtitle: about, icon: GEAR.into(), action: run(&["omarchy-shell", "shell", "summon", id]), stamp, names: Vec::new(), parts });
    }
    docs
}

/// Runnable commands the menu does not already offer.
fn command_docs(cmds: &[Cmd], in_menu: &HashSet<&str>) -> Vec<Doc> {
    cmds.iter()
        .filter(|c| !c.hidden && !c.requires_sudo && !c.args.contains('<') && c.group != "dev")
        .filter(|c| !in_menu.contains(c.binary.as_str()) && VERBS.iter().any(|v| c.summary.starts_with(v)))
        .map(|c| {
            let title = c.summary.trim_end_matches('.').to_string();
            let (stamp, parts) = text_part(&title, c.route.clone());
            Doc {
                key: format!("setting:{}", c.binary),
                kind: Kind::Setting,
                subtitle: c.route.clone(),
                icon: GEAR.into(),
                action: run(&[&c.binary]),
                names: vec![c.route.trim_start_matches("omarchy ").into()],
                title,
                stamp,
                parts,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = r#"{
  // Omarchy menu definition.
  "trigger": {"icon":"󱓞","label":"Trigger"},
  "trigger.emoji": {"icon":"","label":"Emoji","aliases":["emoji","emojis"],"action":"omarchy-menu-emoji"},
  "trigger.toggle": {"icon":"󰔎","label":"Toggle","aliases":"toggle"},
  "trigger.toggle.nightlight": {"icon":"󰔎","label":"Nightlight","action":"omarchy-toggle-nightlight"},
  "trigger.hardware": {"label":"Hardware"},
  "trigger.hardware.laptop-display": {"label":"Laptop Display","when":"omarchy-hw-laptop","action":"omarchy-hyprland-monitor-internal toggle"},
  "style": {"label":"Style"},
  "style.background": {"label":"Background","aliases":["background","wallpaper"],"action":"background=$(omarchy-theme-bg-switcher); [[ -n $background ]] && omarchy-theme-bg-set \"$background\""},
  "setup": {"label":"Setup","when":"false"},
  "setup.network": {"label":"Network"},
  "setup.network.dns": {"label":"DNS","action":"omarchy-dns DHCP"},
  "learn": {"label":"Learn", "target": "style"},
}"#;
    const USER: &str = r#"{
  // "personal": {"label":"Personal"},
  "trigger.toggle.nightlight": {"label":"Night light", "description": "Warm the screen at night"},
}"#;

    fn docs() -> Vec<Doc> {
        let items = menu_items(&[MENU.into(), USER.into()]);
        let failed: HashSet<&str> = ["trigger.hardware.laptop-display", "setup"].into();
        let summaries: HashMap<&str, &str> = [("omarchy-toggle-nightlight", "Toggle nightlight screen temperature"), ("omarchy-theme-bg-set", "Set the current background image"), ("omarchy-theme-bg-switcher", "Open the Omarchy background switcher")].into();
        menu_docs(&items, &failed, &summaries)
    }

    fn embed(d: &Doc) -> &str {
        let ss_core::Embed::Text(t) = &d.parts[0].embed else { panic!() };
        t
    }

    #[test]
    fn jsonc_comments_and_trailing_commas() {
        // Like Omarchy's regex, a ", ]" inside a string loses its comma too.
        assert_eq!(strip_jsonc("{\n  // x\n  \"a\": [1, 2,],\n  \"b\": \"x, ]\",\n}"), "{\n  \"a\": [1, 2],\n  \"b\": \"x ]\"\n}");
    }

    #[test]
    fn menu_rows_with_hidden_branches_dropped() {
        let docs = docs();
        let keys: Vec<&str> = docs.iter().map(|d| d.key.as_str()).collect();
        // Hardware has nothing left once Laptop Display fails its `when:`; Setup fails its own.
        assert_eq!(keys, ["setting:learn", "setting:style", "setting:style.background", "setting:trigger", "setting:trigger.emoji", "setting:trigger.toggle", "setting:trigger.toggle.nightlight"]);
        let night = &docs[6];
        assert_eq!((night.title.as_str(), night.subtitle.as_str(), night.icon.as_str()), ("Night light", "Trigger › Toggle", "󰔎"));
        assert_eq!(embed(night), "title: Night light | text: Trigger › Toggle › Night light. Warm the screen at night. Toggle nightlight screen temperature");
        assert_eq!(night.names, ["Toggle Night light"]);
        assert_eq!(night.action, run(&["bash", "-lc", "omarchy-toggle-nightlight"]));
        let bg = &docs[2];
        assert_eq!(bg.names, ["background", "wallpaper", "Style Background"]);
        assert!(embed(bg).ends_with("Open the Omarchy background switcher. Set the current background image"), "{}", embed(bg));
        assert_eq!(docs[0].action, run(&["omarchy-menu", "summon", "style"]));
        assert_eq!(docs[5].action, run(&["omarchy-menu", "summon", "trigger.toggle"]));
        assert_eq!(docs[5].names, ["toggle", "Trigger Toggle"]);
        assert_eq!(embed(&docs[3]), "title: Trigger | text: Trigger. Includes Emoji, Toggle");
        assert_eq!(docs[4].action, run(&["bash", "-lc", "omarchy-menu-emoji"]));
    }

    #[test]
    fn conditions_run_in_parallel_within_a_time_limit() {
        let marker = format!("ss-sources-hang-{}", std::process::id());
        let hang = format!("sleep 30 # {marker}");
        let t = Instant::now();
        let ok = passing(&["true", "false", "[[ 1 == 1 ]] # comment", "exit 3", &hang, "sleep 0.2"], Duration::from_millis(800));
        assert_eq!(ok, [0, 2, 5].into());
        assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
        // The hung check is killed, not left running.
        let alive = || Command::new("pgrep").args(["-f", &marker]).stdout(Stdio::null()).status().is_ok_and(|s| s.success());
        for _ in 0..20 {
            if !alive() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive());
    }

    #[test]
    fn everyday_phrases_become_names_and_text() {
        let mut docs = docs();
        docs.iter_mut().for_each(add_phrases);
        let night = &docs[6];
        assert_eq!(night.names[..3], ["Toggle Night light", "night light", "blue light filter"]);
        assert!(embed(night).ends_with("Toggle nightlight screen temperature. night light, blue light filter, night mode, night shift, make the screen warmer"), "{}", embed(night));
        assert_eq!(night.stamp, crate::sha1(embed(night)));
        assert_eq!(embed(night).strip_prefix("title: Night light | text: "), Some(night.parts[0].text.as_str()));
        // Rows without phrases keep their text.
        assert_eq!(embed(&docs[3]), "title: Trigger | text: Trigger. Includes Emoji, Toggle");
    }

    #[test]
    fn panels_in_the_bar_only() {
        let shell: Value = serde_json::from_str(r#"{"bar":{"layout":{"left":[{"id":"omarchy.menu"}],"right":[{"id":"omarchy.network"},{"id":"omarchy.tray"}]}}}"#).unwrap();
        let m = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        let manifests = [
            m(r#"{"id":"omarchy.network","name":"Network","description":"Wi-Fi list and connection state","kinds":["bar-widget"],"barWidget":{"displayName":"Network","category":"Network"}}"#),
            m(r#"{"id":"omarchy.bluetooth","name":"Bluetooth","description":"Bluetooth devices","kinds":["bar-widget"]}"#),
            m(r#"{"id":"omarchy.menu","name":"Omarchy menu","kinds":["menu","bar-widget"]}"#),
            m(r#"{"id":"omarchy.clipboard","name":"Clipboard","kinds":["overlay"]}"#),
        ];
        let docs = panel_docs(&shell, &manifests);
        assert_eq!(docs.len(), 1);
        assert_eq!((docs[0].key.as_str(), docs[0].subtitle.as_str()), ("setting:omarchy.network", "Wi-Fi list and connection state"));
        assert_eq!(docs[0].action, run(&["omarchy-shell", "shell", "summon", "omarchy.network"]));
        assert_eq!(embed(&docs[0]), "title: Network | text: Wi-Fi list and connection state. Opens the Network panel");
    }

    #[test]
    fn commands_that_a_person_runs() {
        let cmds: Vec<Cmd> = serde_json::from_str(
            r#"[
            {"route":"omarchy restart wifi","binary":"omarchy-restart-wifi","group":"restart","summary":"Unblock and restart the Wi-Fi service.","args":"","hidden":false,"requires_sudo":false},
            {"route":"omarchy restart audio","binary":"omarchy-restart-audio","group":"restart","summary":"Restart audio services and recover stuck USB audio devices.","args":"","hidden":false,"requires_sudo":false},
            {"route":"omarchy toggle nightlight","binary":"omarchy-toggle-nightlight","group":"toggle","summary":"Toggle nightlight screen temperature","args":"[--status]","hidden":false,"requires_sudo":false},
            {"route":"omarchy theme bg next","binary":"omarchy-theme-bg-next","group":"theme","summary":"Cycle to the next background for the current theme","args":"","hidden":false,"requires_sudo":false},
            {"route":"omarchy theme set","binary":"omarchy-theme-set","group":"theme","summary":"Toggle a theme","args":"<theme-name>","hidden":false,"requires_sudo":false},
            {"route":"omarchy battery present","binary":"omarchy-battery-present","group":"battery","summary":"Returns true if a battery is present.","args":"","hidden":false,"requires_sudo":false},
            {"route":"omarchy toggle secret","binary":"omarchy-toggle-secret","group":"toggle","summary":"Toggle secret","args":"","hidden":true,"requires_sudo":false},
            {"route":"omarchy toggle sudo","binary":"omarchy-toggle-sudo","group":"toggle","summary":"Toggle sudo","args":"","hidden":false,"requires_sudo":true},
            {"route":"omarchy dev status","binary":"omarchy-dev-status","group":"dev","summary":"Show the current dev state","args":"","hidden":false,"requires_sudo":false}
        ]"#,
        )
        .unwrap();
        let docs = command_docs(&cmds, &["omarchy-toggle-nightlight"].into());
        let keys: Vec<&str> = docs.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, ["setting:omarchy-restart-audio", "setting:omarchy-theme-bg-next"]);
        assert_eq!(docs[1].title, "Cycle to the next background for the current theme");
        assert_eq!(docs[1].names, ["theme bg next"]);
        assert_eq!(docs[1].action, run(&["omarchy-theme-bg-next"]));
    }
}
