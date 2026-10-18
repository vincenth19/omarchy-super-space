//! superspace: the daemon, CLI and MCP server in one binary. See DESIGN.md.
//!
//! superspace serve | search "q" [--json] [--kind k1,k2] [--limit N] | read KEY [--part N] [--json]
//! | similar KEY | explain "q" KEY | open KEY ["q"] | pref <op> … | prefs | status | mcp | bench [PARTS]

mod bench;
mod client;
mod embed;
mod ingest;
mod prefs;
mod search;
mod server;
mod store;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

/// Where everything lives. Each path can be moved with an environment variable,
/// so tests and the Pi smoke run never touch the real index.
#[derive(Clone, Debug)]
pub struct Cfg {
    /// The home folder that is indexed (SUPERSPACE_HOME, default $HOME).
    pub home: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub sock: PathBuf,
    pub query_url: String,
    pub index_url: String,
}

impl Cfg {
    pub fn from_env() -> Cfg {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let user = var("HOME").unwrap_or_else(|| "/".into());
        let xdg = |k: &str, rel: &str| var(k).unwrap_or_else(|| user.join(rel)).join("superspace");
        let url = |k: &str, port: u16| std::env::var(k).unwrap_or(format!("http://127.0.0.1:{port}/v1/embeddings"));
        Cfg {
            home: var("SUPERSPACE_HOME").unwrap_or_else(|| user.clone()),
            state: var("SUPERSPACE_STATE").unwrap_or_else(|| xdg("XDG_STATE_HOME", ".local/state")),
            cache: var("SUPERSPACE_CACHE").unwrap_or_else(|| xdg("XDG_CACHE_HOME", ".cache")),
            config: var("SUPERSPACE_CONFIG").unwrap_or_else(|| xdg("XDG_CONFIG_HOME", ".config")),
            sock: var("SUPERSPACE_SOCK")
                .unwrap_or_else(|| var("XDG_RUNTIME_DIR").unwrap_or_else(std::env::temp_dir).join("superspace.sock")),
            query_url: url("SUPERSPACE_QUERY_URL", 47801),
            index_url: url("SUPERSPACE_INDEX_URL", 47802),
        }
    }

    pub fn prefs(&self) -> PathBuf {
        self.config.join("preferences.toml")
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Cfg::from_env();
    let done = match args.first().map(String::as_str) {
        Some("serve") => server::serve(cfg),
        Some("mcp") => client::mcp(&cfg),
        Some("bench") => bench::run(&cfg, args.get(1).and_then(|n| n.parse().ok()).unwrap_or(50_000)).map(|_| ()),
        Some(_) => client::cli(&cfg, &args),
        None => Err(anyhow::anyhow!("usage: {}", USAGE)),
    };
    if let Err(e) = done {
        eprintln!("superspace: {e:#}");
        std::process::exit(1);
    }
}

pub const USAGE: &str = "superspace serve | search \"q\" [--json] [--kind k1,k2] [--limit N] | read KEY [--part N] [--json] \
| similar KEY | explain \"q\" KEY | open KEY [\"q\"] | pref <pin|unpin|boost|bury|exclude|prefer|un…> … | prefs | status | mcp";
