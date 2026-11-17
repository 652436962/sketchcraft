//! Undo history that survives another program's save, one client's steps at a time.
//!
//! Normally an undo step is a copy of the model as it was before the step (fast, exact, and the
//! only thing a single client needs). Those copies are only valid while nobody else changes the file:
//! going back to one after another program saved would erase that program's work. So when a reload
//! brings in someone else's save, each step is turned into a `Patch`: exactly what that step changed
//! (entity by entity), and undoing a patch puts back only those entities, on top of the model as it is
//! now. A step can be undone only if everything it touched is still as the step left it and nothing
//! built on it since; otherwise Undo refuses with a message, and nothing is guessed. A refused step stays
//! in the list (the other program may undo its change), and asking again skips it.
//!
//! The conversion is lazy: a reload only pairs each model copy with the one after it (`Step::Stale`, no
//! work, no copying), and the changes are worked out when that very step is undone, so a big model
//! never freezes on another program's save and no step is let go for being slow.

use crate::model::Model;
use serde_json::Value;
use std::sync::Arc;

/// One entry of the undo (or redo) list.
pub enum Step {
    /// the model before the step (a step taken since the last reload)
    Snap(Box<Model>),
    /// what the step changed (a step that was carried across another program's save)
    Patch(Patch),
    /// a step taken before another program's save, not yet turned into a `Patch`: the model before the step and
    /// the model after it (the next step's "before", or what was on screen); the changes are worked out when it
    /// is undone (or redone)
    Stale { before: Arc<Model>, after: Arc<Model> },
}

impl Step {
    pub fn approx_bytes(&self) -> usize {
        match self {
            Step::Snap(m) => m.approx_bytes(),
            Step::Patch(p) => p.bytes,
            Step::Stale { before, .. } => before.approx_bytes(),
        }
    }
}

/// What one step changed: per top-level part of the model (points, edges, faces, groups, ...),
/// the entries that differ, with their value before and after (`None`: not there).
pub struct Patch {
    parts: Vec<(String, Change)>,
    bytes: usize,
}

enum Change {
    /// a part that is one value (the units, a list of views...): replaced whole
    Whole(Option<Value>, Option<Value>),
    /// a part that is a map by id or name: only the entries that changed
    Keys(Vec<(String, Option<Value>, Option<Value>)>),
}

/// The ids and names change counters don't count as a change of the model's content.
const SKIP: [&str; 1] = ["next_id"];

fn norm(v: Option<&Value>) -> Option<Value> {
    v.filter(|v| !v.is_null()).cloned()
}

/// Two stored values are the same, give or take the last digit of a number (a coordinate that went through a
/// file and back can come out one unit in the last place away).
fn close(a: &Option<Value>, b: &Option<Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => values_close(x, y),
        _ => false,
    }
}

fn values_close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y || (x - y).abs() <= 1e-9 * (1.0 + x.abs().max(y.abs())),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_close(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| values_close(p, q))),
        _ => a == b,
    }
}

fn size_of(v: &Option<Value>) -> usize {
    v.as_ref().map_or(0, |v| v.to_string().len())
}

impl Patch {
    /// What changed from `before` to `after` (both as JSON objects).
    pub fn between(before: &Value, after: &Value) -> Patch {
        let (empty, ob, oa) = (serde_json::Map::new(), before.as_object(), after.as_object());
        let (ob, oa) = (ob.unwrap_or(&empty), oa.unwrap_or(&empty));
        let mut names: Vec<&String> = ob.keys().chain(oa.keys()).collect();
        names.sort();
        names.dedup();
        let mut parts = vec![];
        let mut bytes = 64;
        for name in names {
            if SKIP.contains(&name.as_str()) {
                continue;
            }
            let (b, a) = (norm(ob.get(name)), norm(oa.get(name)));
            if a == b {
                continue;
            }
            // maps keyed by id (points, edges, faces, groups, components...) are compared entry by entry
            let both_maps = b.as_ref().is_none_or(Value::is_object) && a.as_ref().is_none_or(Value::is_object);
            if both_maps {
                let em = serde_json::Map::new();
                let (bm, am) = (b.as_ref().and_then(Value::as_object).unwrap_or(&em), a.as_ref().and_then(Value::as_object).unwrap_or(&em));
                let mut keys: Vec<&String> = bm.keys().chain(am.keys()).collect();
                keys.sort();
                keys.dedup();
                let mut changes = vec![];
                for k in keys {
                    let (kb, ka) = (norm(bm.get(k)), norm(am.get(k)));
                    if kb != ka {
                        bytes += k.len() + size_of(&kb) + size_of(&ka) + 48;
                        changes.push((k.clone(), kb, ka));
                    }
                }
                parts.push((name.clone(), Change::Keys(changes)));
            } else {
                bytes += name.len() + size_of(&b) + size_of(&a) + 48;
                parts.push((name.clone(), Change::Whole(b, a)));
            }
        }
        Patch { parts, bytes }
    }

    /// No entry changed.
    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(|(_, c)| matches!(c, Change::Keys(k) if k.is_empty()))
    }

    /// Put `forward` (false: the step's "before") on top of `current`. Err names what stands in the way.
    pub fn apply(&self, current: &Model, forward: bool) -> Result<Model, String> {
        let mut v = serde_json::to_value(current).map_err(|e| e.to_string())?;
        let Some(obj) = v.as_object_mut() else { return Err("the model couldn't be read".into()) };
        let mut conflicts: Vec<String> = vec![];
        for (name, change) in &self.parts {
            // what the step left there (to be replaced) and what it should become
            let pick = |before: &Option<Value>, after: &Option<Value>| if forward { (before.clone(), after.clone()) } else { (after.clone(), before.clone()) };
            match change {
                Change::Whole(b, a) => {
                    let (expect, to) = pick(b, a);
                    if !close(&norm(obj.get(name)), &expect) {
                        conflicts.push(name.clone());
                        continue;
                    }
                    match to {
                        Some(t) => obj.insert(name.clone(), t),
                        None => obj.remove(name),
                    };
                }
                Change::Keys(keys) => {
                    let mut map = match obj.get(name) {
                        Some(Value::Object(m)) => m.clone(),
                        None | Some(Value::Null) => serde_json::Map::new(),
                        Some(_) => {
                            conflicts.push(name.clone());
                            continue;
                        }
                    };
                    for (k, b, a) in keys {
                        let (expect, to) = pick(b, a);
                        if !close(&norm(map.get(k)), &expect) {
                            conflicts.push(format!("{name} {k}"));
                            continue;
                        }
                        match to {
                            Some(t) => map.insert(k.clone(), t),
                            None => map.remove(k),
                        };
                    }
                    // (an empty map stays an empty object: some parts are required when the model is read back)
                    obj.insert(name.clone(), Value::Object(map));
                }
            }
        }
        if !conflicts.is_empty() {
            return Err(conflicts_text(&conflicts));
        }
        let model: Model = serde_json::from_value(v).map_err(|e| format!("the result wasn't a model ({e})"))?;
        model.check_sane().map_err(|e| e.to_string())?;
        // nothing may be left pointing at something the step took away (another program built on it): a point, an
        // edge, a group, a material... Problems the model already had before are not this step's doing.
        let had = current.broken_refs(true);
        if let Some(what) = model.broken_refs(true).into_iter().find(|w| !had.contains(w)) {
            return Err(format!("{what} now depends on it"));
        }
        Ok(model)
    }
}

fn conflicts_text(c: &[String]) -> String {
    let shown: Vec<&str> = c.iter().take(3).map(String::as_str).collect();
    let more = if c.len() > 3 { format!(" and {} more", c.len() - 3) } else { String::new() };
    format!("{}{more} changed since", shown.join(", "))
}

/// The undo and redo lists of an engine, carried across a reload.
pub struct History {
    pub undo: Vec<Step>,
    pub redo: Vec<Step>,
    /// steps that couldn't be carried over
    pub dropped: usize,
}

/// Turn the undo and redo lists (as they are with `current` on screen) into ones that survive a reload.
/// Nothing is worked out here: each model copy is paired with the one that follows it (`Step::Stale`), and
/// the changes are found when that step is undone. Steps already patches stay as they are.
pub fn carry(undo: Vec<Step>, redo: Vec<Step>, current: &Model) -> History {
    let mut dropped = 0;
    let mut cur: Option<Arc<Model>> = None;
    let mut now = || cur.get_or_insert_with(|| Arc::new(current.clone())).clone();
    // undo: [oldest .. newest]. A copy's "after" is the next copy's "before", or what is on screen.
    enum Item {
        Copy(Arc<Model>),
        Other(Step),
    }
    let items: Vec<Item> = undo
        .into_iter()
        .map(|s| match s {
            Step::Snap(m) => Item::Copy(Arc::new(*m)),
            other => Item::Other(other),
        })
        .collect();
    let mut out_undo: Vec<Step> = vec![];
    for i in 0..items.len() {
        match &items[i] {
            Item::Other(Step::Patch(p)) => out_undo.push(Step::Patch(p.clone_patch())),
            Item::Other(Step::Stale { before, after }) => out_undo.push(Step::Stale { before: before.clone(), after: after.clone() }),
            Item::Other(Step::Snap(_)) => {}
            Item::Copy(before) => {
                let after = match items.get(i + 1) {
                    None => Some(now()),
                    Some(Item::Copy(n)) => Some(n.clone()),
                    // a patch above a copy: what the copy led to is unknown
                    Some(Item::Other(_)) => None,
                };
                match after {
                    Some(after) => out_undo.push(Step::Stale { before: before.clone(), after }),
                    None => dropped += 1,
                }
            }
        }
    }
    // redo: [bottom .. next]. From the next step down, each copy leads to the one under it; past a patch the model
    // isn't known, so copies below one can't be paired.
    let mut out_redo: Vec<Step> = vec![];
    let mut prev: Option<Arc<Model>> = Some(now());
    for s in redo.into_iter().rev() {
        match s {
            Step::Snap(m) => {
                let next = Arc::new(*m);
                match prev.take() {
                    Some(before) => {
                        out_redo.push(Step::Stale { before, after: next.clone() });
                        prev = Some(next);
                    }
                    None => dropped += 1,
                }
            }
            Step::Patch(p) => {
                out_redo.push(Step::Patch(p));
                prev = None;
            }
            Step::Stale { before, after } => {
                out_redo.push(Step::Stale { before, after });
                prev = None;
            }
        }
    }
    // out_redo was built next-first; the list keeps the next step last
    out_redo.reverse();
    History { undo: out_undo, redo: out_redo, dropped }
}

impl Patch {
    fn clone_patch(&self) -> Patch {
        Patch { parts: self.parts.iter().map(|(n, c)| (n.clone(), clone_change(c))).collect(), bytes: self.bytes }
    }
}

fn clone_change(c: &Change) -> Change {
    match c {
        Change::Whole(a, b) => Change::Whole(a.clone(), b.clone()),
        Change::Keys(k) => Change::Keys(k.clone()),
    }
}

/// The changes between two models, as a patch (empty when the step changed nothing in the model's content).
pub fn patch_between(before: &Model, after: &Model) -> Patch {
    let b = serde_json::to_value(before).unwrap_or(Value::Null);
    let a = serde_json::to_value(after).unwrap_or(Value::Null);
    Patch::between(&b, &a)
}
