//! A model shared through a file: whoever changes it saves it, everyone else
//! reloads when the file's timestamp moves. That's how the app, the browser
//! bridge and Claude (over MCP) all edit one model at once.
//!
//! The file is the one place a person's work lives, so it is handled
//! carefully:
//!  - it is written whole-or-not-at-all (temp file, flush, rename);
//!  - a model with NaN or absurd numbers is never written;
//!  - the previous version is kept in `Backups/` beside it (a few, rolling);
//!  - a file that can't be read is copied aside and reported, never silently
//!    replaced by an empty model;
//!  - before every command we pick up what another client saved, so two
//!    clients don't overwrite each other's work.

use crate::policy::Caller;
use crate::safety::write_atomic;
use crate::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How many of the newest backups of the live file are always kept. Older
/// ones are thinned (one per hour for two days, then one per day) rather than
/// dropped, up to `MAX_BACKUPS`, so a burst of activity can't push out the history.
const KEEP_BACKUPS: usize = 8;
const MAX_BACKUPS: usize = 60;
/// Damaged copies (`.damaged-*`) kept beside the live file.
const KEEP_DAMAGED: usize = 5;
/// How long a notice about a damaged file stays on the scene.
const NOTICE_FOR: Duration = Duration::from_secs(600);
/// A backup is taken at most this often while working (and always before `new`/`open`).
const BACKUP_EVERY: Duration = Duration::from_secs(300);

pub struct Live {
    pub engine: Engine,
    pub doc: PathBuf,
    seen: Option<SystemTime>,
    /// no file: an in-memory model (an agent with no --doc)
    persist: bool,
    last_backup: Option<SystemTime>,
    /// something the person should know (a damaged file was set aside, a save failed)
    pub notice: Option<String>,
    notice_at: Option<SystemTime>,
    /// saving is off: the file couldn't be read and couldn't be copied aside
    blocked: bool,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn epoch(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn default_doc() -> PathBuf {
    if let Ok(p) = std::env::var("SKETCHCRAFT_DOC") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents/SketchCraft/live.sketchcraft")
}

impl Live {
    pub fn open(doc: PathBuf) -> Self {
        let mut live = Live { engine: Engine::new(), doc, seen: None, persist: true, last_backup: None, notice: None, notice_at: None, blocked: false };
        if live.doc.exists() {
            live.load(true);
        }
        live.seen = mtime(&live.doc);
        live
    }

    /// A model that is not saved anywhere.
    pub fn memory(engine: Engine) -> Self {
        Live { engine, doc: PathBuf::new(), seen: None, persist: false, last_backup: None, notice: None, notice_at: None, blocked: false }
    }

    /// Open the file into the engine. If it can't be read, set it aside (a copy
    /// that is never overwritten) and say so; `at_start`: the engine stays empty.
    fn load(&mut self, at_start: bool) -> bool {
        // a reload keeps the component being edited, if it is still there
        let ctx = self.engine.ctx;
        let r = self.engine.exec_json(&json!({"cmd": "open", "path": self.doc}).to_string());
        if r["ok"] == true {
            if let Some(d) = ctx.filter(|d| self.engine.model.definitions.contains_key(d)) {
                self.engine.ctx = Some(d);
            }
            // the file reads fine now: whatever was wrong is over
            self.blocked = false;
            self.notice = None;
            return true;
        }
        let why = r["error"].as_str().unwrap_or("unknown error").to_string();
        let aside = self.unique_damaged_path();
        match std::fs::copy(&self.doc, &aside) {
            Ok(_) => {
                self.blocked = false;
                self.set_notice(format!("The model file couldn't be opened ({why}). A copy was kept at {}; this session started from {}.", aside.display(), if at_start { "an empty model" } else { "the model in memory" }));
                self.prune_damaged();
            }
            Err(e) => {
                self.blocked = true;
                self.set_notice(format!("The model file couldn't be opened ({why}) and couldn't be copied aside ({e}), so nothing will be saved over it. Repair or move {} and it will be picked up again.", self.doc.display()));
            }
        }
        false
    }

    fn set_notice(&mut self, text: String) {
        self.notice = Some(text);
        self.notice_at = Some(SystemTime::now());
    }

    fn unique_damaged_path(&self) -> PathBuf {
        let t = epoch(SystemTime::now());
        let mut n = 0;
        loop {
            let p = self.doc.with_extension(format!("damaged-{t}{}", if n == 0 { String::new() } else { format!("-{n}") }));
            if !p.exists() {
                return p;
            }
            n += 1;
        }
    }

    /// Keep only the newest few `.damaged-*` copies.
    fn prune_damaged(&self) {
        let (Some(dir), Some(stem)) = (self.doc.parent().filter(|d| !d.as_os_str().is_empty()).or(Some(Path::new("."))), self.doc.file_stem().map(|s| s.to_string_lossy().into_owned())) else { return };
        let prefix = format!("{stem}.damaged-");
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix))).collect()).unwrap_or_default();
        all.sort_by_key(|p| (mtime(p), p.clone()));
        while all.len() > KEEP_DAMAGED {
            std::fs::remove_file(all.remove(0)).ok();
        }
    }

    /// Run a command as the person using the app; save the model if it changed.
    pub fn exec(&mut self, cmd: &Value) -> Value {
        self.exec_as(&Caller::Ui, cmd)
    }

    pub fn exec_as(&mut self, caller: &Caller, cmd: &Value) -> Value {
        // someone else may have saved since we last looked: start from theirs
        self.sync();
        let name = cmd["cmd"].as_str().unwrap_or("");
        let (before, replaces) = (self.engine.revision(), matches!(name, "new" | "open"));
        let mut r = self.engine.exec_value_as(caller, cmd.clone());
        if r["ok"] == true && self.engine.revision() != before && self.persist {
            // another client saved while this command ran: theirs wins, ours is dropped,
            // so nobody's work is overwritten (run the command again)
            if self.doc.exists() && !self.blocked && mtime(&self.doc) != self.seen {
                self.seen = mtime(&self.doc);
                if self.load(false) {
                    return json!({"ok": false, "error": "Someone else saved the model while that ran, so their version was kept and that change was not applied. Run it again."});
                }
            }
            match self.save(replaces) {
                Ok(()) => {
                    if self.notice.as_deref().is_some_and(|n| n.starts_with("Couldn't save")) {
                        self.notice = None;
                    }
                }
                Err(e) => {
                    self.set_notice(format!("Couldn't save your model: {e}"));
                    r["save_error"] = json!(e);
                }
            }
        }
        r
    }

    /// Write the model to the live file (atomically), keeping backups.
    pub fn save(&mut self, force_backup: bool) -> Result<(), String> {
        if self.blocked {
            return Err("saving is off because the model file couldn't be read (see the notice)".into());
        }
        self.engine.model.check_sane().map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(&self.engine.model).map_err(|e| e.to_string())?;
        if let Some(dir) = self.doc.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        self.backup(force_backup);
        write_atomic(&self.doc, &bytes).map_err(|e| e.to_string())?;
        self.seen = mtime(&self.doc);
        Ok(())
    }

    /// Keep the file as it is now in `Backups/`, if the last backup is old
    /// enough (or `force`). A file that is blank, unreadable or identical to
    /// the newest backup is not worth a slot, and thinning (see `thin`) keeps
    /// older history, so repeated `new`/`open` can't push out the last good copy.
    fn backup(&mut self, force: bool) {
        let Some(dir) = self.doc.parent().map(|d| d.join("Backups")) else { return };
        let now = SystemTime::now();
        let due = force || self.last_backup.is_none_or(|t| now.duration_since(t).unwrap_or_default() >= BACKUP_EVERY);
        if !due || !self.doc.is_file() {
            return;
        }
        let Ok(bytes) = std::fs::read(&self.doc) else { return };
        match serde_json::from_slice::<crate::model::Model>(&bytes) {
            Ok(m) if !m.is_blank() => {}
            _ => return,
        }
        let stem = self.doc.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "live".into());
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let mine = |p: &PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{stem}-")));
        let mut all: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(mine).collect()).unwrap_or_default();
        all.sort_by_key(|p| backup_key(p));
        if all.last().is_some_and(|newest| std::fs::read(newest).is_ok_and(|b| b == bytes)) {
            self.last_backup = Some(now);
            return;
        }
        let mut n = 0;
        let target = loop {
            let t = dir.join(format!("{stem}-{}{}.sketchcraft", epoch(now), if n == 0 { String::new() } else { format!("-{n}") }));
            if !t.exists() {
                break t;
            }
            n += 1;
        };
        if std::fs::write(&target, &bytes).is_ok() {
            self.last_backup = Some(now);
            all.push(target);
            for p in thin(&all) {
                std::fs::remove_file(p).ok();
            }
        }
    }

    /// Reload if someone else saved since we last looked. True if reloaded.
    pub fn sync(&mut self) -> bool {
        if !self.persist {
            return false;
        }
        let now = mtime(&self.doc);
        if now.is_some() && now != self.seen {
            self.seen = now;
            return self.load(false);
        }
        false
    }

    pub fn scene(&self) -> Value {
        let mut s = crate::scene::build(&self.engine.model, self.engine.ctx);
        s["doc"] = json!(self.doc);
        if let (Some(n), Some(at)) = (&self.notice, self.notice_at) {
            if SystemTime::now().duration_since(at).unwrap_or_default() < NOTICE_FOR || self.blocked {
                s["notice"] = json!(n);
            }
        }
        s
    }
}

/// Which backups (oldest first) to delete: the newest `KEEP_BACKUPS` stay; of
/// older ones the first of each 30 s (last 10 min), 5 min (last hour), hour (two days) and day
/// stay, up to `MAX_BACKUPS` in all (the oldest go first).
fn thin(all: &[PathBuf]) -> Vec<PathBuf> {
    let mut keep = vec![false; all.len()];
    let cut = all.len().saturating_sub(KEEP_BACKUPS);
    let now = epoch(SystemTime::now());
    let mut seen = std::collections::HashSet::new();
    for (i, p) in all.iter().enumerate() {
        if i >= cut {
            keep[i] = true;
            continue;
        }
        let t = backup_key(p).0;
        let age = now.saturating_sub(t);
        let bucket = match age {
            0..600 => (0u8, t / 30),
            600..3600 => (1, t / 300),
            3600..172_800 => (2, t / 3600),
            _ => (3, t / 86400),
        };
        keep[i] = seen.insert(bucket);
    }
    let mut kept = keep.iter().filter(|k| **k).count();
    let mut out = vec![];
    for (i, p) in all.iter().enumerate() {
        if !keep[i] {
            out.push(p.clone());
        } else if kept > MAX_BACKUPS {
            kept -= 1;
            out.push(p.clone());
        }
    }
    out
}

/// (seconds, counter) from a backup's name `stem-<seconds>[-<n>].sketchcraft`, so
/// backups made in the same second still sort in the order they were taken.
fn backup_key(p: &Path) -> (u64, u64) {
    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut parts = name.rsplit('-');
    let last = parts.next().and_then(|x| x.parse::<u64>().ok());
    match (last, parts.next().and_then(|x| x.parse::<u64>().ok())) {
        (Some(n), Some(t)) => (t, n),
        (Some(t), _) => (t, 0),
        _ => (0, 0),
    }
}
