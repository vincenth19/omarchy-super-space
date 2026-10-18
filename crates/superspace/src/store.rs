//! The index: SQLite on disk (items, parts, FTS5 over titles and part text,
//! picks) and, in memory, item metadata plus one contiguous int8 matrix of
//! part vectors. Vectors are int8 with one scale per row (768 bytes a part,
//! 38 MB for 50k parts) so the daemon stays under its 150 MB budget.

use crate::embed::{DIM, quantize};
use crate::search::QVec;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use ss_core::{Action, Doc, Embed, Kind, Locator, Part};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

/// Bump to read every file and source again (vectors are kept for unchanged text).
const INDEX_VERSION: &str = "2";
/// Bump when the model or vector format changes: the index starts over.
const MODEL: &str = "embeddinggemma-2-q8:768:i8";

const SCHEMA: &str = "
pragma journal_mode = wal;
pragma synchronous = normal;
create table if not exists meta (k text primary key, v text not null);
create table if not exists items (id integer primary key, key text unique not null, kind text not null,
  title text not null, subtitle text not null, icon text not null, action text not null, names text not null,
  stamp text not null, mtime integer not null, added integer not null);
-- input is the pending model input ('' for an image); null once embedded or refused.
-- vec is int8 x 768 when embedded, empty when the model refused the part.
create table if not exists parts (id integer primary key, item integer not null, locator text not null,
  image text, input text, prio integer not null, ihash integer not null, vec blob, scale real);
create index if not exists parts_item on parts (item);
create index if not exists parts_pending on parts (prio, id) where input is not null;
create virtual table if not exists fts using fts5 (title, text, tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3 4');
create table if not exists picks (key text not null, q text not null, ts integer not null);
create table if not exists prefer (text text primary key, vec blob not null);
";

pub struct Item {
    pub id: i64,
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    pub action: Action,
    pub names: Vec<String>,
    /// File modification time (unix seconds), 0 when not a file.
    pub mtime: i64,
    /// When the item was first indexed.
    pub added: i64,
    pub parts: Vec<i64>,
}

pub struct PartMeta {
    pub item: i64,
    pub locator: Locator,
    /// The picture this part was embedded from (a photo or video frame).
    pub image: Option<String>,
    /// Row in the vector matrix, once embedded.
    pub row: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Pick {
    pub key: String,
    pub q: String,
    pub ts: i64,
}

/// What a part needs to be embedded.
pub struct Pending {
    pub id: i64,
    pub text: String,
    pub image: Option<String>,
}

#[derive(Default)]
pub struct Mem {
    pub items: HashMap<i64, Item>,
    pub keys: HashMap<String, i64>,
    pub parts: HashMap<i64, PartMeta>,
    /// Row r is vecs[r*DIM..(r+1)*DIM], part rows[r], cosine scale scales[r].
    pub vecs: Vec<i8>,
    pub scales: Vec<f32>,
    pub rows: Vec<i64>,
    /// Sum of all rows (dequantized): the corpus centroid, for hub scores.
    pub sum: Vec<f32>,
}

impl Mem {
    pub fn vec(&self, row: usize) -> &[i8] {
        &self.vecs[row * DIM..(row + 1) * DIM]
    }

    fn add_to_sum(&mut self, row: usize, sign: f32) {
        let s = self.scales[row] * sign;
        if self.sum.len() != DIM {
            self.sum = vec![0.0; DIM];
        }
        for (acc, &x) in self.sum.iter_mut().zip(&self.vecs[row * DIM..(row + 1) * DIM]) {
            *acc += x as f32 * s;
        }
    }

    fn set_vec(&mut self, part: i64, v: &[i8], scale: f32) {
        let Some(p) = self.parts.get_mut(&part) else { return };
        let row = match p.row {
            Some(r) => r,
            None => {
                let r = self.rows.len();
                p.row = Some(r);
                self.rows.push(part);
                self.scales.push(0.0);
                self.vecs.resize((r + 1) * DIM, 0);
                r
            }
        };
        self.add_to_sum(row, -1.0);
        self.vecs[row * DIM..(row + 1) * DIM].copy_from_slice(v);
        self.scales[row] = scale;
        self.add_to_sum(row, 1.0);
    }

    fn remove_item(&mut self, id: i64) {
        let Some(item) = self.items.remove(&id) else { return };
        self.keys.remove(&item.key);
        for pid in item.parts {
            let Some(PartMeta { row: Some(r), .. }) = self.parts.remove(&pid) else { continue };
            self.add_to_sum(r, -1.0);
            // Move the last row into the hole.
            let last = self.rows.len() - 1;
            if r != last {
                self.vecs.copy_within(last * DIM..(last + 1) * DIM, r * DIM);
                self.scales[r] = self.scales[last];
                self.rows[r] = self.rows[last];
                if let Some(m) = self.parts.get_mut(&self.rows[r]) {
                    m.row = Some(r);
                }
            }
            self.rows.pop();
            self.scales.pop();
            self.vecs.truncate(last * DIM);
        }
    }

    fn insert(&mut self, mut item: Item, parts: Vec<(i64, PartMeta, Option<QVec>)>) {
        for (pid, meta, vec) in parts {
            item.parts.push(pid);
            self.parts.insert(pid, meta);
            if let Some((v, s)) = vec {
                self.set_vec(pid, &v, s);
            }
        }
        self.keys.insert(item.key.clone(), item.id);
        self.items.insert(item.id, item);
    }
}

pub struct Store {
    path: PathBuf,
    db: Mutex<Connection>,
    readers: Mutex<Vec<Connection>>,
    pub mem: RwLock<Mem>,
    /// Keys of the files the user uses: their parts are embedded before other files'.
    pub used: RwLock<HashSet<String>>,
}

pub fn kind_name(k: Kind) -> String {
    serde_json::to_value(k).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

pub fn parse_kind(s: &str) -> Option<Kind> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// FNV-1a: stable across builds, unlike std's hasher (hashes are stored).
pub fn fnv(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.bytes().chain([0xff]) {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Embedding order, by stage: apps, settings, every document's first part (its
/// overview; the user's documents first), the rest of the user's documents,
/// clips, the rest of the other documents, history, emoji, then photos (and
/// other pictures), songs, videos. Newest-modified first within a stage.
pub fn prio(kind: Kind, image: bool, first: bool, used: bool, mtime: i64) -> i64 {
    let stage = match kind {
        Kind::App => 0,
        Kind::Setting => 1,
        Kind::File if first => 2 + i64::from(!used),
        Kind::File if used => 4,
        Kind::Clip if !image => 5,
        Kind::File => 6,
        Kind::History => 7,
        Kind::Emoji => 8,
        Kind::Audio => 10,
        Kind::Video => 11,
        _ => 9,
    };
    const AGE: i64 = (1 << 40) - 1;
    stage << 40 | (AGE - mtime.clamp(0, AGE))
}

/// What each stage of `prio` is called in `Progress`.
pub const STAGES: [&str; 12] = ["apps and settings", "apps and settings", "documents (gist)", "documents (gist)", "documents",
    "clipboard", "documents", "history", "emoji", "photos", "songs", "videos"];

/// Parts below this priority are "text": media waits until none are pending.
pub const TEXT_PRIO: i64 = 9 << 40;

/// Vectors made ahead of time by `superspace precompute` (data/vectors.bin, compiled in;
/// may be empty): records of fnv(MODEL, input) as u64 LE, the scale as f32 LE and DIM
/// int8s, sorted by hash.
#[cfg(not(test))]
static READY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vectors.bin"));
#[cfg(test)]
static READY: &[u8] = &[];
const REC: usize = 12 + DIM;

/// The ready-made vector (int8 bytes, scale) for this exact model input, if `data` has one.
pub fn lookup(data: &[u8], input: &str) -> Option<(Vec<u8>, f32)> {
    let recs = data.as_chunks::<REC>().0;
    let r = &recs[recs.binary_search_by_key(&fnv(&[MODEL, input]), |r| u64::from_le_bytes(r[..8].try_into().unwrap())).ok()?];
    Some((r[12..].to_vec(), f32::from_le_bytes(r[8..12].try_into().unwrap())))
}

/// One record of a ready-made vector file, with its sort key.
pub fn record(input: &str, v: &[f32]) -> (u64, Vec<u8>) {
    let ((q, s), h) = (quantize(v), fnv(&[MODEL, input]));
    (h, [&h.to_le_bytes()[..], &s.to_le_bytes(), &u8s(&q)].concat())
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn i8s(b: &[u8]) -> Vec<i8> {
    b.iter().map(|&x| x as i8).collect()
}

fn u8s(v: &[i8]) -> Vec<u8> {
    v.iter().map(|&x| x as u8).collect()
}

fn drop_item(db: &Connection, id: i64) -> rusqlite::Result<()> {
    db.execute("delete from fts where rowid in (select id from parts where item = ?1)", [id])?;
    db.execute("delete from parts where item = ?1", [id])?;
    db.execute("delete from items where id = ?1", [id])?;
    Ok(())
}

fn connect(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(10))?;
    Ok(db)
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = connect(path)?;
        db.execute_batch(SCHEMA)?;
        let meta = |k: &str| db.query_row("select v from meta where k = ?1", [k], |r| r.get::<_, String>(0)).optional();
        if meta("model")?.as_deref() != Some(MODEL) {
            db.execute_batch("delete from items; delete from parts; delete from fts; delete from prefer;")?;
        }
        if meta("version")?.as_deref() != Some(INDEX_VERSION) {
            db.execute("update items set stamp = ''", [])?;
        }
        db.execute("insert or replace into meta values ('model', ?1), ('version', ?2)", [MODEL, INDEX_VERSION])?;
        let mem = load(&db)?;
        Ok(Store { path: path.to_path_buf(), db: Mutex::new(db), readers: Mutex::new(Vec::new()), mem: RwLock::new(mem), used: Default::default() })
    }

    /// Run `f` on a pooled read connection (many searches read at once).
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let pooled = self.readers.lock().unwrap().pop();
        let conn = match pooled {
            Some(c) => c,
            None => connect(&self.path)?,
        };
        let out = f(&conn);
        self.readers.lock().unwrap().push(conn);
        out
    }

    /// Add or replace items, keyed by `Doc::key`, with their stamp and file mtime.
    /// A part whose model input did not change keeps its vector (or its refusal);
    /// a new input takes a ready-made vector when there is one.
    pub fn put(&self, docs: Vec<(Doc, String, i64)>) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        let docs: Vec<_> = docs.into_iter().rev().filter(|d| seen.insert(d.0.key.clone())).collect();
        if docs.is_empty() {
            return Ok(());
        }
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let (mut gone, mut fresh) = (Vec::new(), Vec::new());
        for (doc, stamp, mtime) in docs {
            let mut reuse: HashMap<i64, (Vec<u8>, f32)> = HashMap::new();
            let old = tx
                .query_row("select id, added from items where key = ?1", [&doc.key], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?)))
                .optional()?;
            if let Some((old, _)) = old {
                let mut st = tx.prepare_cached("select ihash, vec, scale from parts where item = ?1 and input is null and vec is not null")?;
                for row in st.query_map([old], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<f32>>(2)?.unwrap_or(0.0))))? {
                    let (h, v, s) = row?;
                    reuse.insert(h, (v, s));
                }
                drop_item(&tx, old)?;
                gone.push(old);
            }
            let added = old.map_or_else(now, |o| o.1);
            tx.execute(
                "insert into items (key, kind, title, subtitle, icon, action, names, stamp, mtime, added) values (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![doc.key, kind_name(doc.kind), doc.title, doc.subtitle, doc.icon,
                    serde_json::to_string(&doc.action)?, serde_json::to_string(&doc.names)?, stamp, mtime, added],
            )?;
            let id = tx.last_insert_rowid();
            let fts_title = std::iter::once(&doc.title).chain(&doc.names).cloned().collect::<Vec<_>>().join(" ");
            let parts = if doc.parts.is_empty() {
                vec![Part { text: String::new(), locator: Locator::default(), embed: Embed::None }]
            } else {
                doc.parts
            };
            let (mut metas, mut first, used) = (Vec::new(), true, self.used.read().unwrap().contains(&doc.key));
            for p in parts {
                let (input, image) = match p.embed {
                    Embed::None => (None, None),
                    Embed::Text(t) => (Some(t), None),
                    Embed::Image(path) => (Some(String::new()), Some(path.to_string_lossy().into_owned())),
                };
                let ihash = fnv(&[input.as_deref().unwrap_or(""), image.as_deref().unwrap_or("")]) as i64;
                let kept = input.as_ref().and_then(|t| reuse.get(&ihash).cloned().or_else(|| image.is_none().then(|| lookup(READY, t))?));
                let prio = prio(doc.kind, image.is_some(), input.is_some() && std::mem::take(&mut first), used, mtime);
                tx.execute(
                    "insert into parts (item, locator, image, input, prio, ihash, vec, scale) values (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![id, serde_json::to_string(&p.locator)?, image, if kept.is_some() { None } else { input },
                        prio, ihash, kept.as_ref().map(|k| &k.0), kept.as_ref().map(|k| k.1)],
                )?;
                let pid = tx.last_insert_rowid();
                tx.execute("insert into fts (rowid, title, text) values (?1, ?2, ?3)", params![pid, fts_title, p.text])?;
                let vec = kept.filter(|k| k.0.len() == DIM).map(|k| (i8s(&k.0), k.1));
                metas.push((pid, PartMeta { item: id, locator: p.locator, image, row: None }, vec));
            }
            let item = Item { id, key: doc.key, kind: doc.kind, title: doc.title, subtitle: doc.subtitle, icon: doc.icon,
                action: doc.action, names: doc.names, mtime, added, parts: Vec::new() };
            fresh.push((item, metas));
        }
        tx.commit()?;
        let mut mem = self.mem.write().unwrap();
        for id in gone {
            mem.remove_item(id);
        }
        for (item, metas) in fresh {
            mem.insert(item, metas);
        }
        Ok(())
    }

    /// Forget items by exact key, or every key starting with one of `prefixes`.
    pub fn delete(&self, keys: &[String], prefixes: &[String]) -> Result<usize> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let mut ids = Vec::new();
        for k in keys {
            ids.extend(tx.query_row("select id from items where key = ?1", [k], |r| r.get::<_, i64>(0)).optional()?);
        }
        for p in prefixes {
            let mut st = tx.prepare_cached("select id from items where substr(key, 1, ?2) = ?1")?;
            for id in st.query_map(params![p, p.chars().count() as i64], |r| r.get::<_, i64>(0))? {
                ids.push(id?);
            }
        }
        for &id in &ids {
            drop_item(&tx, id)?;
        }
        tx.commit()?;
        let mut mem = self.mem.write().unwrap();
        for &id in &ids {
            mem.remove_item(id);
        }
        Ok(ids.len())
    }

    /// key → stamp for files ("file:" keys) or for one kind.
    pub fn stamps(&self, kind: Option<Kind>) -> Result<HashMap<String, String>> {
        let db = self.db.lock().unwrap();
        let (sql, arg) = match kind {
            None => ("select key, stamp from items where substr(key, 1, 5) = ?1", "file:".to_string()),
            Some(k) => ("select key, stamp from items where kind = ?1", kind_name(k)),
        };
        let mut st = db.prepare(sql)?;
        let rows = st.query_map([arg], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn stamp(&self, key: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("select stamp from items where key = ?1", [key], |r| r.get(0)).ok()
    }

    /// The next parts waiting for the model, most wanted first.
    pub fn pending(&self, n: usize) -> Result<Vec<Pending>> {
        let db = self.db.lock().unwrap();
        let mut st = db.prepare_cached("select id, input, image from parts where input is not null order by prio, id limit ?1")?;
        let rows = st.query_map([n as i64], |r| Ok(Pending { id: r.get(0)?, text: r.get(1)?, image: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How many parts wait for the model, and the priority of the most wanted
    /// (on a reader connection, so a search never waits for indexing).
    pub fn queue(&self) -> (usize, Option<i64>) {
        let sql = "select count(*), min(prio) from parts where input is not null";
        self.read(|c| c.query_row(sql, [], |r| Ok((r.get::<_, i64>(0)? as usize, r.get(1)?)))).unwrap_or((0, None))
    }

    /// (pending, pending text, refused) part counts.
    pub fn counts(&self) -> (i64, i64, i64) {
        let db = self.db.lock().unwrap();
        let n = |sql: &str| db.query_row(sql, [], |r| r.get(0)).unwrap_or(0);
        (
            n("select count(*) from parts where input is not null"),
            n(&format!("select count(*) from parts where input is not null and prio < {TEXT_PRIO}")),
            n("select count(*) from parts where input is null and length(vec) = 0"),
        )
    }

    /// Store vectors for parts, or mark them refused (None): neither is retried
    /// until the part's text changes.
    pub fn set_vecs(&self, done: &[(i64, Option<QVec>)]) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let mut stored = Vec::new();
        for (id, v) in done {
            let (blob, scale) = v.as_ref().map_or((Vec::new(), 0.0), |(v, s)| (u8s(v), *s));
            let n = tx.execute("update parts set input = null, vec = ?2, scale = ?3 where id = ?1", params![id, blob, scale])?;
            if n == 1 {
                stored.extend(v.as_ref().map(|v| (*id, v)));
            }
        }
        tx.commit()?;
        let mut mem = self.mem.write().unwrap();
        for (id, (v, s)) in stored {
            mem.set_vec(id, v, *s);
        }
        Ok(())
    }

    pub fn add_pick(&self, p: &Pick) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("insert into picks (key, q, ts) values (?1, ?2, ?3)", params![p.key, p.q, p.ts])?;
        db.execute("delete from picks where rowid <= (select max(rowid) - 5000 from picks)", [])?;
        Ok(())
    }

    pub fn picks(&self) -> Result<Vec<Pick>> {
        let db = self.db.lock().unwrap();
        let mut st = db.prepare("select key, q, ts from picks order by rowid")?;
        let rows = st.query_map([], |r| Ok(Pick { key: r.get(0)?, q: r.get(1)?, ts: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Cached embeddings of `prefer` sentences.
    pub fn prefer_vec(&self, text: &str) -> Option<Vec<f32>> {
        let db = self.db.lock().unwrap();
        let blob: Vec<u8> = db.query_row("select vec from prefer where text = ?1", [text], |r| r.get(0)).ok()?;
        Some(blob.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }

    pub fn set_prefer_vec(&self, text: &str, v: &[f32]) -> Result<()> {
        let blob: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let db = self.db.lock().unwrap();
        db.execute("insert or replace into prefer values (?1, ?2)", params![text, blob])?;
        Ok(())
    }
}

/// The text of one part.
pub fn part_text(c: &Connection, part: i64) -> String {
    c.prepare_cached("select text from fts where rowid = ?1")
        .and_then(|mut st| st.query_row([part], |r| r.get(0)))
        .unwrap_or_default()
}

fn load(db: &Connection) -> Result<Mem> {
    let mut mem = Mem::default();
    let mut st = db.prepare("select id, key, kind, title, subtitle, icon, action, names, mtime, added from items")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        let (Some(kind), Ok(action), Ok(names)) = (
            parse_kind(&r.get::<_, String>(2)?),
            serde_json::from_str(&r.get::<_, String>(6)?),
            serde_json::from_str(&r.get::<_, String>(7)?),
        ) else {
            continue;
        };
        let item = Item { id: r.get(0)?, key: r.get(1)?, kind, title: r.get(3)?, subtitle: r.get(4)?, icon: r.get(5)?,
            action, names, mtime: r.get(8)?, added: r.get(9)?, parts: Vec::new() };
        mem.keys.insert(item.key.clone(), item.id);
        mem.items.insert(item.id, item);
    }
    let mut st = db.prepare("select id, item, locator, image, vec, scale from parts order by item, id")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        let (id, item): (i64, i64) = (r.get(0)?, r.get(1)?);
        let Some(it) = mem.items.get_mut(&item) else { continue };
        it.parts.push(id);
        let locator = serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default();
        mem.parts.insert(id, PartMeta { item, locator, image: r.get(3)?, row: None });
        let vec: Option<Vec<u8>> = r.get(4)?;
        if let Some(v) = vec.filter(|v| v.len() == DIM) {
            mem.set_vec(id, &i8s(&v), r.get::<_, Option<f32>>(5)?.unwrap_or(0.0));
        }
    }
    Ok(mem)
}
