//! The CLI and the MCP server: both are clients of the daemon's socket.

use crate::Cfg;
use crate::store::parse_kind;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// Send one request; return its response (the final phase for a search).
pub fn call(cfg: &Cfg, req: &Value) -> Result<Value> {
    let mut s = UnixStream::connect(&cfg.sock)
        .with_context(|| format!("cannot reach the daemon at {} (is `superspace serve` running?)", cfg.sock.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    writeln!(s, "{req}")?;
    // Nothing more will be asked, so the daemon need not wait for typing to calm down.
    s.shutdown(std::net::Shutdown::Write)?;
    for line in BufReader::new(s).lines() {
        let v: Value = serde_json::from_str(&line?)?;
        if v["id"] == req["id"] && v["phase"] != "instant" {
            if let Some(e) = v["error"].as_str() {
                bail!("{e}");
            }
            return Ok(v);
        }
    }
    bail!("the daemon closed the connection")
}

/// A search through the daemon, optionally only some kinds ("file,video").
pub fn search(cfg: &Cfg, q: &str, kinds: Option<&str>, limit: usize) -> Result<Value> {
    let kinds: Option<Vec<&str>> = kinds.map(|k| k.split(',').map(str::trim).collect());
    call(cfg, &json!({"op": "search", "id": 1, "q": q, "limit": limit, "kinds": kinds}))
}

pub fn cli(cfg: &Cfg, args: &[String]) -> Result<()> {
    let (mut pos, mut json_out, mut kinds, mut limit, mut part) = (Vec::new(), false, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json_out = true,
            "--kind" => kinds = it.next().cloned(),
            "--limit" => limit = it.next().and_then(|n| n.parse().ok()),
            "--part" => part = it.next().and_then(|n| n.parse::<usize>().ok()),
            _ => pos.push(a.as_str()),
        }
    }
    let arg = |i: usize| pos.get(i).copied().with_context(|| format!("usage: {}", crate::USAGE));
    let req = |op: &str, more: Value| {
        let mut r = json!({"op": op, "id": 1});
        r.as_object_mut().unwrap().extend(more.as_object().cloned().unwrap_or_default());
        r
    };
    let v = match arg(0)? {
        "search" => search(cfg, &pos[1..].join(" "), kinds.as_deref(), limit.unwrap_or(10))?,
        "read" => call(cfg, &req("read", json!({"key": arg(1)?, "part": part})))?,
        "similar" => call(cfg, &req("similar", json!({"key": arg(1)?, "limit": limit.unwrap_or(10)})))?,
        "explain" => call(cfg, &req("explain", json!({"q": arg(1)?, "key": arg(2)?})))?,
        "open" => call(cfg, &req("activate", json!({"key": arg(1)?, "q": pos.get(2).copied().unwrap_or("")})))?,
        "pref" => call(cfg, &req("pref", json!({"change": pref_change(&pos[1..])?})))?,
        "prefs" => call(cfg, &req("prefs", json!({})))?,
        "status" => call(cfg, &req("status", json!({})))?,
        other => bail!("unknown command {other:?}; usage: {}", crate::USAGE),
    };
    let out = match (arg(0)?, json_out) {
        ("search", false) => human_rows(&v["rows"], v["answer"].as_object().map(|a| (a["input"].clone(), a["output"].clone()))),
        ("similar", false) => human_rows(&v["value"]["rows"], None),
        ("read", false) => {
            let r = &v["value"];
            let mut s = format!("{}  ({})\n", r["title"].as_str().unwrap_or(""), r["key"].as_str().unwrap_or(""));
            for p in r["parts"].as_array().into_iter().flatten() {
                s += &format!("\n[{} {}]\n{}\n", p["n"], p["locator"], p["text"].as_str().unwrap_or(""));
            }
            s
        }
        (_, true) if v.get("value").is_some() => v["value"].to_string(),
        (_, true) => v.to_string(),
        _ => serde_json::to_string_pretty(v.get("value").unwrap_or(&v))?,
    };
    println!("{}", out.trim_end());
    Ok(())
}

fn human_rows(rows: &Value, answer: Option<(Value, Value)>) -> String {
    let mut s = String::new();
    if let Some((i, o)) = answer {
        s += &format!("= {} → {}\n", i.as_str().unwrap_or(""), o.as_str().unwrap_or(""));
    }
    for (n, r) in rows.as_array().into_iter().flatten().enumerate() {
        let txt = |k: &str| r[k].as_str().unwrap_or("").to_string();
        s += &format!("{:>2}. {}  ·  {} {}  ·  {}  [{}]\n", n + 1, txt("title"), txt("kind"), txt("why"), txt("subtitle"), txt("key"));
        if let Some(sn) = r["snippet"]["text"].as_str() {
            s += &format!("      {sn}\n");
        }
    }
    s
}

/// `pref pin "lease" KEY`, `pref boost ~/Documents/Meetup 0.1`, `pref bury clip`,
/// `pref exclude ~/Downloads/old`, `pref prefer "sentence"`, `pref un… …`.
fn pref_change(a: &[&str]) -> Result<Value> {
    let usage = "pref <pin|unpin> Q KEY | <boost|bury|unboost|unbury> PATH|KIND|KEY [BY] | <exclude|unexclude> PATH|KIND|KEY | <prefer|unprefer> SENTENCE";
    let op = *a.first().context(usage)?;
    let rest = a.get(1..).unwrap_or_default();
    let target = |t: &str| {
        let field = if parse_kind(t).is_some() { "kind" } else if t.contains(':') && !t.starts_with(['/', '~']) { "key" } else { "path" };
        json!({ field: t })
    };
    let val = match op.trim_start_matches("un") {
        "pin" if rest.len() == 2 => json!({"q": rest[0], "key": rest[1]}),
        "pin" if op == "unpin" && rest.len() == 1 => json!({"q": rest[0]}),
        "boost" | "bury" | "exclude" if !rest.is_empty() => {
            let mut v = target(rest[0]);
            if let Some(by) = rest.get(1) {
                v["by"] = json!(by.parse::<f64>().context("BY is a number, e.g. 0.1")?);
            }
            v
        }
        "prefer" if !rest.is_empty() => json!(rest.join(" ")),
        _ => bail!("usage: {usage}"),
    };
    Ok(json!({ op: val }))
}

fn tools() -> Value {
    let s = |props: Value, req: &[&str]| json!({"type": "object", "properties": props, "required": req});
    json!([
        {"name": "search", "description": "Search the user's computer by name, keyword and meaning: files (and the text inside PDFs and Office files, with pages), folders by name, photos, video moments, spoken words, apps, settings, clipboard, browser history. Use this instead of ls/find/grep for the user's files. Results are ranked; empty=true means nothing local matched.",
         "inputSchema": s(json!({"q": {"type": "string"}, "kinds": {"type": "array", "items": {"type": "string", "enum": ["app","setting","file","folder","image","video","audio","clip","emoji","history","web"]}}, "limit": {"type": "integer", "default": 10}}), &["q"])},
        {"name": "read", "description": "Read an indexed item's text by key (from search), all parts or one part (0-based).",
         "inputSchema": s(json!({"key": {"type": "string"}, "part": {"type": "integer"}}), &["key"])},
        {"name": "similar", "description": "Items closest in meaning to an item.",
         "inputSchema": s(json!({"key": {"type": "string"}, "limit": {"type": "integer", "default": 10}}), &["key"])},
        {"name": "explain", "description": "Every signal behind an item's rank for a query (why it is or is not shown).",
         "inputSchema": s(json!({"q": {"type": "string"}, "key": {"type": "string"}}), &["q", "key"])},
        {"name": "prefs_get", "description": "The user's search preferences (pins, boosts, burials, exclusions, prefer sentences, cutoffs).",
         "inputSchema": s(json!({}), &[])},
        {"name": "pref", "description": "Change one preference. change is one of {\"pin\":{\"q\":…,\"key\":…}}, {\"boost\"|\"bury\":{\"path\"|\"kind\"|\"key\":…,\"by\":0.1}}, {\"exclude\":{\"path\"|\"kind\"|\"key\":…}}, {\"prefer\":\"sentence\"}; prefix un to remove (unpin, unboost, …).",
         "inputSchema": s(json!({"change": {"type": "object"}}), &["change"])},
    ])
}

/// Rows as an agent needs them: no icons, short hits.
fn compact(v: &Value) -> Value {
    let rows: Vec<Value> = v["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let mut c = json!({"key": r["key"], "kind": r["kind"], "title": r["title"], "subtitle": r["subtitle"], "score": r["score"], "why": r["why"]});
            // History URLs go to agents without their query and fragment
            // (login and reset tokens live there); the box still opens the full URL.
            let url = r["action"]["url"].as_str().map(|u| if r["kind"] == "history" { u.split(['?', '#']).next().unwrap_or(u) } else { u });
            if let Some(p) = r["action"]["path"].as_str().or(url) {
                c["open"] = json!(p);
            }
            if let Some(t) = r["snippet"]["text"].as_str() {
                c["snippet"] = json!(t);
            }
            let hits: Vec<Value> = r["hits"].as_array().into_iter().flatten().take(4).map(|h| json!({"at": h["locator"], "text": h["text"]})).collect();
            if !hits.is_empty() {
                c["hits"] = json!(hits);
            }
            c
        })
        .collect();
    let mut out = json!({"rows": rows});
    for k in ["empty", "answer", "filter"] {
        if !v[k].is_null() {
            out[k] = v[k].clone();
        }
    }
    out
}

fn tool_call(cfg: &Cfg, p: &Value) -> Value {
    let a = &p["arguments"];
    let limit = a["limit"].as_u64().unwrap_or(10) as usize;
    let out = match p["name"].as_str().unwrap_or("") {
        "search" => {
            let kinds = a["kinds"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","));
            search(cfg, a["q"].as_str().unwrap_or(""), kinds.as_deref(), limit).map(|v| compact(&v))
        }
        "read" => call(cfg, &json!({"op": "read", "id": 1, "key": a["key"], "part": a["part"]})).map(|v| v["value"].clone()),
        "similar" => call(cfg, &json!({"op": "similar", "id": 1, "key": a["key"], "limit": limit})).map(|v| compact(&v["value"])),
        "explain" => call(cfg, &json!({"op": "explain", "id": 1, "q": a["q"], "key": a["key"]})).map(|v| v["value"].clone()),
        "prefs_get" => call(cfg, &json!({"op": "prefs", "id": 1})).map(|v| v["value"].clone()),
        "pref" => call(cfg, &json!({"op": "pref", "id": 1, "change": a["change"]})).map(|v| v["value"].clone()),
        other => Err(anyhow::anyhow!("unknown tool {other:?}")),
    };
    match out {
        Ok(v) => {
            // Under 10k tokens: about 30k characters.
            let mut text = v.to_string();
            if text.len() > 30_000 {
                text = text.chars().take(30_000).collect::<String>() + "… (truncated; ask for fewer rows or one part)";
            }
            json!({"content": [{"type": "text", "text": text}]})
        }
        Err(e) => json!({"content": [{"type": "text", "text": format!("{e:#}")}], "isError": true}),
    }
}

pub fn mcp(cfg: &Cfg) -> Result<()> {
    mcp_loop(cfg, std::io::stdin().lock(), std::io::stdout())
}

/// MCP over stdio: JSON-RPC 2.0, one message per line.
pub fn mcp_loop(cfg: &Cfg, input: impl BufRead, mut out: impl Write) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}))?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // a notification
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "superspace", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Search the user's own files, apps and history with these tools before using ls/find/grep.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(tool_call(cfg, &msg["params"])),
            m => Err(json!({"code": -32601, "message": format!("method not found: {m}")})),
        };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        };
        writeln!(out, "{resp}")?;
        out.flush()?;
    }
    Ok(())
}
