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
/// A backup taken just before something destructive (`new`, `open`, a delete or any change that
/// drops a quarter of the model) is kept this long whatever else is thinned.
const PRE_KEEP_SECS: u64 = 86_400;
/// Destructive commands that take a backup first (at most this often, unless they shrink the model a lot).
const DESTRUCTIVE: [&str; 8] = ["new", "open", "delete", "purge", "explode", "ungroup", "clear_guides", "delete_view"];
const DESTRUCTIVE_EVERY: Duration = Duration::from_secs(15);
/// Backups whose names say they were made further ahead than this are not ours: ignored, never thinned.
const CLOCK_SLACK_SECS: u64 = 120;
/// Files kept when memory was set aside because another program saved first.
const KEEP_UNSAVED: usize = 5;

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
    /// commands ran inside a batch and the file doesn't have them yet
    unsaved_batch: bool,
    /// saving is off: the file couldn't be read and couldn't be copied aside
    blocked: bool,
    /// memory holds work the file doesn't (a save failed): never replaced without keeping it first
    dirty: bool,
    /// the side file that holds work set aside; the notice about it stays while the file does
    sticky: Option<PathBuf>,
    last_destructive: Option<SystemTime>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn epoch(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// True when this process is a Rust test program: a unit test (`cfg!(test)`), a program cargo put in
/// a `deps/` folder (every test program is built there, whatever the target folder is called: a
/// custom `CARGO_TARGET_DIR` works), or one that sets `SKETCHCRAFT_TESTING`. The shipped programs
/// run from a `release/` folder or an install folder, never from `deps/`.
pub fn running_as_cargo_test() -> bool {
    if cfg!(test) || std::env::var_os("SKETCHCRAFT_TESTING").is_some_and(|v| !v.is_empty() && v != "0") {
        return true;
    }
    std::env::current_exe().ok().is_some_and(|p| p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "deps"))
}

/// Where the person's model folder is. `SKETCHCRAFT_DOC` always wins (and scopes every folder
/// beside it: Library, Plugins, Backups). In a test program with no `SKETCHCRAFT_DOC`, a throwaway
/// folder in the temp directory, so `cargo test` can never touch the real Documents folder.
pub fn doc_path(var: Option<std::ffi::OsString>, testing: bool, home: Option<PathBuf>) -> PathBuf {
    if let Some(p) = var {
        return PathBuf::from(p);
    }
    if testing {
        return std::env::temp_dir().join(format!("sketchcraft-test-{}", std::process::id())).join("live.sketchcraft");
    }
    home.unwrap_or_else(|| PathBuf::from(".")).join("Documents/SketchCraft/live.sketchcraft")
}

pub fn default_doc() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    doc_path(std::env::var_os("SKETCHCRAFT_DOC"), running_as_cargo_test(), home)
}

impl Live {
    pub fn open(doc: PathBuf) -> Self {
        let mut live = Live { engine: Engine::new(), doc, seen: None, persist: true, last_backup: None, notice: None, notice_at: None, unsaved_batch: false, blocked: false, dirty: false, sticky: None, last_destructive: None };
        if live.doc.exists() {
            live.load(true);
        }
        live.seen = mtime(&live.doc);
        live
    }

    /// A model that is not saved anywhere.
    pub fn memory(engine: Engine) -> Self {
        Live { engine, doc: PathBuf::new(), seen: None, persist: false, last_backup: None, notice: None, notice_at: None, unsaved_batch: false, blocked: false, dirty: false, sticky: None, last_destructive: None }
    }

    /// Open the file into the engine. If it can't be read, set it aside (a copy
    /// that is never overwritten) and say so; `at_start`: the engine stays empty.
    fn load(&mut self, at_start: bool) -> bool {
        // a file that holds exactly what memory holds (a touch, a sync tool, our own save seen late)
        // is not a change: the undo history stays
        if !at_start && !self.dirty {
            if let (Ok(file), Ok(mem)) = (std::fs::read(&self.doc), serde_json::to_vec(&self.engine.model)) {
                if file == mem {
                    return true;
                }
            }
        }
        // a reload keeps the component being edited, if it is still there
        let ctx = self.engine.ctx;
        let had_history = self.engine.undo_depth() > 0;
        let r = self.engine.exec_json(&json!({"cmd": "open", "path": self.doc}).to_string());
        if r["ok"] == true {
            if let Some(d) = ctx.filter(|d| self.engine.model.definitions.contains_key(d)) {
                self.engine.ctx = Some(d);
            }
            // the file reads fine now: whatever was wrong is over
            self.blocked = false;
            self.unsaved_batch = false;
            self.dirty = false;
            if self.sticky.is_none() {
                self.notice = None;
                if had_history {
                    // the person's own steps can't be kept: undoing past another program's save would erase its work
                    self.set_notice("Another program saved this model, so Undo can only go back to that point.".into());
                }
            }
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

    /// Keep only the newest few `<stem>.<kind>-*` side copies.
    fn prune_side(&self, kind: &str, keep: usize) {
        let (Some(dir), Some(stem)) = (self.doc.parent().filter(|d| !d.as_os_str().is_empty()).or(Some(Path::new("."))), self.doc.file_stem().map(|s| s.to_string_lossy().into_owned())) else { return };
        let prefix = format!("{stem}.{kind}-");
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix))).collect()).unwrap_or_default();
        all.sort_by_key(|p| (mtime(p), p.clone()));
        while all.len() > keep {
            std::fs::remove_file(all.remove(0)).ok();
        }
    }

    /// Keep only the newest few `.damaged-*` copies.
    fn prune_damaged(&self) {
        self.prune_side("damaged", KEEP_DAMAGED);
    }

    /// Set the model in memory aside (`<name>.unsaved-<time>.sketchcraft` beside the file) before
    /// something replaces it, and keep telling the person until that file is dealt with.
    fn stash(&mut self, why: &str) -> Result<PathBuf, String> {
        let t = epoch(SystemTime::now());
        let mut n = 0;
        let path = loop {
            let p = self.doc.with_extension(format!("unsaved-{t}{}.sketchcraft", if n == 0 { String::new() } else { format!("-{n}") }));
            if !p.exists() {
                break p;
            }
            n += 1;
        };
        let bytes = serde_json::to_vec(&self.engine.model).map_err(|e| e.to_string())?;
        write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
        self.prune_side("unsaved", KEEP_UNSAVED);
        self.sticky = Some(path.clone());
        self.set_notice(format!("{why} Your changes were kept in {} (open it to get them back). This message stays until that file is moved or deleted.", path.display()));
        Ok(path)
    }

    /// Another client saved. Replace what is in memory with their file, after setting aside any work
    /// memory holds that the file doesn't (`ours_ahead`, or a failed save). False if the file wasn't taken.
    fn take_theirs(&mut self, ours_ahead: bool, why: &str) -> bool {
        if ours_ahead || self.dirty {
            if let Err(e) = self.stash(why) {
                self.set_notice(format!("{why} Your unsaved changes couldn't be set aside ({e}), so the other version was not loaded."));
                return false;
            }
        }
        self.load(false)
    }

    /// Housekeeping that must not wait for the next command: close a batch nobody closed (saving
    /// what it did), and forget a side-file notice once the file is gone.
    fn tick(&mut self) {
        if self.sticky.as_ref().is_some_and(|p| !p.exists()) {
            self.sticky = None;
            self.notice = None;
        }
        if self.persist && self.engine.close_expired_batch() {
            self.finish_batch();
        }
    }

    /// Save what an unclosed batch did (no reply to give anyone).
    fn finish_batch(&mut self) {
        if !self.unsaved_batch {
            return;
        }
        self.unsaved_batch = false;
        if self.doc.exists() && !self.blocked && mtime(&self.doc) != self.seen {
            self.seen = mtime(&self.doc);
            self.take_theirs(true, "The plugin run was cut off and another program had saved the model meanwhile.");
            return;
        }
        if let Err(e) = self.save(false) {
            self.dirty = true;
            self.set_notice(format!("Couldn't save your model: {e}"));
        }
    }

    /// Run a command as the person using the app; save the model if it changed.
    pub fn exec(&mut self, cmd: &Value) -> Value {
        self.exec_as(&Caller::Ui, cmd)
    }

    pub fn exec_as(&mut self, caller: &Caller, cmd: &Value) -> Value {
        self.tick();
        // someone else may have saved since we last looked: start from theirs. Never inside a batch (a
        // plugin run): the rest of its commands would land on their model; theirs is picked up when it ends.
        if !self.engine.in_batch() {
            self.sync();
        }
        let name = cmd["cmd"].as_str().unwrap_or("");
        fn weight(m: &crate::model::Model) -> usize {
            m.faces.len() + m.instances.len() + m.groups.len() + m.definitions.values().map(|d| weight(&d.model)).sum::<usize>()
        }
        let size = |e: &Engine| weight(&e.model);
        let (before, size_before) = (self.engine.revision(), size(&self.engine));
        let destructive = DESTRUCTIVE.contains(&name);
        let mut r = self.engine.exec_value_as(caller, cmd.clone());
        // inside a batch (a plugin run) the file is written once, when the batch ends
        let changed = r["ok"] == true && self.engine.revision() != before;
        if self.engine.in_batch() {
            self.unsaved_batch |= changed;
            return r;
        }
        let had_batch = self.unsaved_batch;
        let changed = changed || (r["ok"] == true && self.unsaved_batch);
        self.unsaved_batch = false;
        if changed && self.persist {
            // a change that drops a quarter of the model (or a destructive command) keeps the file as it
            // was in Backups first: the person's own undo doesn't cross a reload, so this is the way back
            let shrank = !matches!(name, "undo" | "redo") && size_before > 0 && size(&self.engine) * 4 < size_before * 3;
            let paced = self.last_destructive.is_none_or(|t| SystemTime::now().duration_since(t).unwrap_or_default() >= DESTRUCTIVE_EVERY);
            let force = matches!(name, "new" | "open") || shrank || (destructive && paced);
            if force && !matches!(name, "new" | "open") {
                self.last_destructive = Some(SystemTime::now());
            }
            // another client saved while this command ran: theirs wins, so nobody's work is overwritten.
            // A plugin run's whole output is set aside first (a single command is just run again)
            if self.doc.exists() && !self.blocked && mtime(&self.doc) != self.seen {
                self.seen = mtime(&self.doc);
                let why = "Another program saved the model while a plugin was running.";
                let kept = if had_batch { self.stash(why).ok() } else { None };
                if had_batch && kept.is_none() {
                    return json!({"ok": false, "error": "Another program saved the model while the plugin ran, and the plugin's changes couldn't be set aside, so nothing was saved. Run it again."});
                }
                if self.load(false) {
                    return json!({"ok": false, "error": match kept {
                        Some(p) => format!("Someone else saved the model while that ran, so their version was kept. What the plugin made was set aside in {}.", p.display()),
                        None => "Someone else saved the model while that ran, so their version was kept and that change was not applied. Run it again.".into(),
                    }});
                }
            }
            match self.save(force) {
                Ok(()) => {
                    self.dirty = false;
                    if self.notice.as_deref().is_some_and(|n| n.starts_with("Couldn't save")) {
                        self.notice = None;
                    }
                }
                Err(e) => {
                    self.dirty = true;
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
    /// A forced backup is a "pre" one: kept a day whatever else is thinned.
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
        let mut all = list_backups(&dir, &stem, epoch(now));
        if all.last().is_some_and(|newest| std::fs::read(&newest.path).is_ok_and(|b| b == bytes)) {
            self.last_backup = Some(now);
            return;
        }
        let mut n = 0;
        let t = epoch(now);
        let target = loop {
            let tail = if n == 0 { String::new() } else { format!("-{n}") };
            let plain = dir.join(format!("{stem}-{t}{tail}.sketchcraft"));
            let pre = dir.join(format!("{stem}-{t}{tail}-pre.sketchcraft"));
            if !plain.exists() && !pre.exists() {
                break if force { pre } else { plain };
            }
            n += 1;
        };
        // whole or not at all: a crash or a full disk must not leave a half file that counts as a slot
        if write_atomic(&target, &bytes).is_ok() {
            self.last_backup = Some(now);
            all = list_backups(&dir, &stem, epoch(now));
            for p in thin(&all, epoch(now)) {
                std::fs::remove_file(p).ok();
            }
        }
    }

    /// Reload if someone else saved since we last looked. True if reloaded. Not inside a batch
    /// (a plugin run): theirs is picked up when it ends.
    pub fn sync(&mut self) -> bool {
        if !self.persist {
            return false;
        }
        self.tick();
        if self.engine.in_batch() {
            return false;
        }
        let now = mtime(&self.doc);
        if now.is_some() && now != self.seen {
            if self.blocked || self.dirty {
                // memory may hold work the file doesn't: set it aside first (and don't reload if that fails)
                let ok = self.take_theirs(false, "Another program saved this model while yours couldn't be saved.");
                if ok {
                    self.seen = now;
                }
                return ok;
            }
            self.seen = now;
            return self.load(false);
        }
        false
    }

    pub fn scene(&self) -> Value {
        let mut s = crate::scene::build(&self.engine.model, self.engine.ctx);
        s["doc"] = json!(self.doc);
        if let (Some(n), Some(at)) = (&self.notice, self.notice_at) {
            if SystemTime::now().duration_since(at).unwrap_or_default() < NOTICE_FOR || self.blocked || self.dirty || self.sticky.as_ref().is_some_and(|p| p.exists()) {
                s["notice"] = json!(n);
            }
        }
        s
    }
}

/// A backup SketchCraft wrote: its name is exactly `<stem>-<seconds>[-<n>][-pre].sketchcraft`.
#[derive(Clone, Debug)]
struct Backup {
    path: PathBuf,
    secs: u64,
    n: u64,
    pre: bool,
    size: u64,
}

/// What `name` says about itself if it is one of the backups of the document `stem` (and nothing
/// else: `a-b-170...` is not a backup of `a`).
fn parse_backup(stem: &str, name: &str) -> Option<(u64, u64, bool)> {
    let rest = name.strip_prefix(stem)?.strip_prefix('-')?.strip_suffix(".sketchcraft")?;
    let (rest, pre) = match rest.strip_suffix("-pre") {
        Some(r) => (r, true),
        None => (rest, false),
    };
    let mut parts = rest.split('-');
    let secs = parts.next().filter(|d| (9..=11).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))?.parse::<u64>().ok()?;
    let n = match parts.next() {
        None => 0,
        Some(d) if !d.is_empty() && d.len() <= 6 && d.bytes().all(|b| b.is_ascii_digit()) => d.parse().ok()?,
        _ => return None,
    };
    parts.next().is_none().then_some((secs, n, pre))
}

/// The backups of `stem` in `dir`, oldest first. Only real files with SketchCraft's own names;
/// a name claiming a time well in the future (`now` plus a couple of minutes: a clock that was
/// set back, or a file put there by something else) is ignored, so it can't push out real ones.
fn list_backups(dir: &Path, stem: &str, now: u64) -> Vec<Backup> {
    let mut all: Vec<Backup> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let meta = std::fs::symlink_metadata(e.path()).ok().filter(|m| m.is_file() && m.len() > 0)?;
                    let (secs, n, pre) = parse_backup(stem, &e.file_name().to_string_lossy())?;
                    (secs <= now + CLOCK_SLACK_SECS).then(|| Backup { path: e.path(), secs, n, pre, size: meta.len() })
                })
                .collect()
        })
        .unwrap_or_default();
    all.sort_by(|a, b| (a.secs, a.n, a.pre).cmp(&(b.secs, b.n, b.pre)));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_never_default_to_the_real_folder() {
        // no SKETCHCRAFT_DOC, in a test program: a throwaway folder, whatever HOME is
        let p = doc_path(None, true, Some(PathBuf::from("/Users/someone")));
        assert!(p.starts_with(std::env::temp_dir()), "{p:?}");
        assert!(!p.to_string_lossy().contains("Documents/SketchCraft"));
        // outside tests it is the person's folder, and SKETCHCRAFT_DOC wins in both
        assert_eq!(doc_path(None, false, Some(PathBuf::from("/Users/someone"))), PathBuf::from("/Users/someone/Documents/SketchCraft/live.sketchcraft"));
        assert_eq!(doc_path(Some("/x/y.sketchcraft".into()), true, None), PathBuf::from("/x/y.sketchcraft"));
        // and this very process is recognized as a test program
        assert!(running_as_cargo_test());
    }

    fn b(secs: u64, n: u64, pre: bool, size: u64) -> Backup {
        Backup { path: PathBuf::from(format!("b-{secs}-{n}{}", if pre { "-pre" } else { "" })), secs, n, pre, size }
    }

    #[test]
    fn backup_names_match_only_their_own_document() {
        assert_eq!(parse_backup("a", "a-1794862562.sketchcraft"), Some((1794862562, 0, false)));
        assert_eq!(parse_backup("a", "a-1794862562-3-pre.sketchcraft"), Some((1794862562, 3, true)));
        // another document whose name starts with this one's, and names that aren't ours
        assert_eq!(parse_backup("a", "a-b-1794862562.sketchcraft"), None);
        assert_eq!(parse_backup("a-b", "a-b-1794862562.sketchcraft"), Some((1794862562, 0, false)));
        assert_eq!(parse_backup("a", "a-draft.sketchcraft"), None);
        assert_eq!(parse_backup("a", "a-12.sketchcraft"), None);
        assert_eq!(parse_backup("a", "a-1794862562.skp"), None);
        assert_eq!(parse_backup("a", "a-1794862562-1-2.sketchcraft"), None);
    }

    #[test]
    fn thinning_keeps_the_newest_of_a_bucket_and_what_was_taken_before_a_destructive_change() {
        let now = 1_800_000_000;
        // twenty backups inside one 30 s bucket: the newest eight, plus the newest of the bucket (one of those)
        let mut all: Vec<Backup> = (0..20).map(|i| b(now - 100 + i, 0, false, 10)).collect();
        // the good one, older and taken just before a `new`
        all.insert(0, b(now - 100, 1, true, 5000));
        all.sort_by(|x, y| (x.secs, x.n, x.pre).cmp(&(y.secs, y.n, y.pre)));
        let gone: Vec<String> = thin(&all, now).iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert!(!gone.iter().any(|g| g.contains("-pre")), "the pre-destructive backup was thinned: {gone:?}");
        assert!(!gone.iter().any(|g| g == &format!("b-{}-0", now - 81)), "newest 8 kept");
        assert!(gone.len() >= 8, "the older plain ones are thinned: {gone:?}");
        // more than MAX_BACKUPS: the oldest go, the newest never
        let many: Vec<Backup> = (0..200).map(|i| b(now - 200_000 + i * 1000, 0, false, 10)).collect();
        let gone = thin(&many, now);
        assert!(!gone.contains(&many[199].path));
        assert!(many.len() - gone.len() <= MAX_BACKUPS);
    }
}

/// Which backups (given oldest first) to delete. Kept: the newest `KEEP_BACKUPS`; of the older
/// ones the newest of each 30 s (last 10 min), 5 min (last hour), hour (two days) and day; every
/// "pre" backup (taken just before a destructive change) for a day; and the biggest of the last day.
/// At most `MAX_BACKUPS` in all: when there are more, the oldest unprotected go first.
fn thin(all: &[Backup], now: u64) -> Vec<PathBuf> {
    let mut keep = vec![false; all.len()];
    let cut = all.len().saturating_sub(KEEP_BACKUPS);
    let mut seen = std::collections::HashSet::new();
    // newest first, so the first seen in a bucket is the newest in it
    for (i, b) in all.iter().enumerate().rev() {
        if i >= cut {
            keep[i] = true;
            continue;
        }
        let t = b.secs;
        let age = now.saturating_sub(t);
        let bucket = match age {
            0..600 => (0u8, t / 30),
            600..3600 => (1, t / 300),
            3600..172_800 => (2, t / 3600),
            _ => (3, t / 86400),
        };
        keep[i] = seen.insert(bucket);
    }
    let day = |b: &Backup| now.saturating_sub(b.secs) < PRE_KEEP_SECS;
    let mut protected = vec![false; all.len()];
    for (i, b) in all.iter().enumerate() {
        if b.pre && day(b) {
            protected[i] = true;
        }
    }
    if let Some((i, _)) = all.iter().enumerate().filter(|(_, b)| day(b)).max_by_key(|(_, b)| b.size) {
        protected[i] = true;
    }
    for i in 0..all.len() {
        keep[i] |= protected[i];
    }
    let mut kept = keep.iter().filter(|k| **k).count();
    let mut drop = vec![false; all.len()];
    for (i, k) in keep.iter().enumerate() {
        drop[i] = !*k;
    }
    // over the limit: the oldest unprotected go first, then the oldest protected (never the newest)
    for pass in [false, true] {
        for i in 0..all.len().saturating_sub(1) {
            if kept <= MAX_BACKUPS {
                break;
            }
            if keep[i] && !drop[i] && (pass || !protected[i]) && i < cut.max(1) {
                drop[i] = true;
                kept -= 1;
            }
        }
    }
    all.iter().zip(drop).filter(|(_, d)| *d).map(|(b, _)| b.path.clone()).collect()
}
