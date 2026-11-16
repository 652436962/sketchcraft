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

/// How many timestamped backups of the live file are kept.
const KEEP_BACKUPS: usize = 8;
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
        let mut live = Live { engine: Engine::new(), doc, seen: None, persist: true, last_backup: None, notice: None, blocked: false };
        if live.doc.exists() {
            live.load(true);
        }
        live.seen = mtime(&live.doc);
        live
    }

    /// A model that is not saved anywhere.
    pub fn memory(engine: Engine) -> Self {
        Live { engine, doc: PathBuf::new(), seen: None, persist: false, last_backup: None, notice: None, blocked: false }
    }

    /// Open the file into the engine. If it can't be read, set it aside (a copy
    /// that is never overwritten) and say so; `at_start`: the engine stays empty.
    fn load(&mut self, at_start: bool) -> bool {
        let r = self.engine.exec_json(&json!({"cmd": "open", "path": self.doc}).to_string());
        if r["ok"] == true {
            return true;
        }
        let why = r["error"].as_str().unwrap_or("unknown error").to_string();
        let aside = self.doc.with_extension(format!("damaged-{}", epoch(SystemTime::now())));
        match std::fs::copy(&self.doc, &aside) {
            Ok(_) => {
                self.notice = Some(format!("The model file couldn't be opened ({why}). A copy was kept at {}; this session started from {}.", aside.display(), if at_start { "an empty model" } else { "the model in memory" }));
            }
            Err(e) => {
                self.blocked = true;
                self.notice = Some(format!("The model file couldn't be opened ({why}) and couldn't be copied aside ({e}), so nothing will be saved over it. Move or repair {} and restart.", self.doc.display()));
            }
        }
        false
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
            if let Err(e) = self.save(replaces) {
                self.notice = Some(format!("Couldn't save your model: {e}"));
                r["save_error"] = json!(e);
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
    /// enough (or `force`), and drop the oldest beyond `KEEP_BACKUPS`.
    fn backup(&mut self, force: bool) {
        let Some(dir) = self.doc.parent().map(|d| d.join("Backups")) else { return };
        let now = SystemTime::now();
        let due = force || self.last_backup.is_none_or(|t| now.duration_since(t).unwrap_or_default() >= BACKUP_EVERY);
        if !due || !self.doc.is_file() {
            return;
        }
        let stem = self.doc.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "live".into());
        if std::fs::create_dir_all(&dir).is_err() {
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
        if std::fs::copy(&self.doc, &target).is_ok() {
            self.last_backup = Some(now);
            let mut all: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{stem}-")))).collect()).unwrap_or_default();
            all.sort_by_key(|p| (mtime(p), p.clone()));
            while all.len() > KEEP_BACKUPS {
                std::fs::remove_file(all.remove(0)).ok();
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
        if let Some(n) = &self.notice {
            s["notice"] = json!(n);
        }
        s
    }
}
