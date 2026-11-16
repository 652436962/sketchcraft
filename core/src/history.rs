//! Undo history that survives another program's save, one client's steps at a time.
//!
//! Normally an undo step is a copy of the model as it was before the step (fast, exact, and the
//! only thing a single client needs). Those copies are only valid while nobody else changes the file:
//! going back to one after another program saved would erase that program's work. So when a reload
//! brings in someone else's save, each step is turned into a `Patch`: exactly what that step changed
//! (entity by entity), and undoing a patch puts back only those entities, on top of the model as it is
//! now. A step can be undone only if everything it touched is still as the step left it and nothing
//! built on it since; otherwise Undo refuses with a message and drops that step, and nothing is guessed.

use crate::model::Model;
use serde_json::Value;
use std::collections::HashSet;

/// One entry of the undo (or redo) list.
pub enum Step {
    /// the model before the step (a step taken since the last reload)
    Snap(Box<Model>),
    /// what the step changed (a step that was carried across another program's save)
    Patch(Patch),
}

impl Step {
    pub fn approx_bytes(&self) -> usize {
        match self {
            Step::Snap(m) => m.approx_bytes(),
            Step::Patch(p) => p.bytes,
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
                    if norm(obj.get(name)) != expect {
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
                        if norm(map.get(k)) != expect {
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
        // nothing may be left pointing at something the step took away (another program built on it)
        if let Some(what) = dangling(&model) {
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

/// Something in the model that refers to a thing that isn't there, if anything does.
fn dangling(m: &Model) -> Option<String> {
    let edge_keys: HashSet<(u64, u64)> = m.edges.values().map(|e| (e.a.min(e.b) as u64, e.a.max(e.b) as u64)).collect();
    for (id, e) in &m.edges {
        if !m.vertices.contains_key(&e.a) || !m.vertices.contains_key(&e.b) {
            return Some(format!("edge {id}"));
        }
    }
    for (id, f) in &m.faces {
        for l in f.loops() {
            if l.iter().any(|v| !m.vertices.contains_key(v)) {
                return Some(format!("face {id}"));
            }
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if !edge_keys.contains(&(a.min(b) as u64, a.max(b) as u64)) {
                    return Some(format!("face {id}"));
                }
            }
        }
        if f.group.is_some_and(|g| !m.groups.contains_key(&g)) {
            return Some(format!("face {id}"));
        }
    }
    for (id, i) in &m.instances {
        if !m.definitions.contains_key(&i.def) || i.parent.is_some_and(|g| !m.groups.contains_key(&g)) {
            return Some(format!("placement {id}"));
        }
    }
    for (id, g) in &m.groups {
        if g.parent.is_some_and(|p| !m.groups.contains_key(&p)) {
            return Some(format!("group {id}"));
        }
    }
    None
}

/// The undo and redo lists of an engine, carried across a reload.
pub struct History {
    pub undo: Vec<Step>,
    pub redo: Vec<Step>,
    /// steps that couldn't be carried over
    pub dropped: usize,
}

/// How long turning the history into patches may take before the older steps are let go.
const CONVERT_SECONDS: u64 = 4;

/// Turn the undo and redo lists (as they are with `current` on screen) into ones that survive a reload.
/// Steps already patches are kept; model copies become patches, newest first, until the time is up.
pub fn carry(undo: &[Step], redo: &[Step], current: &Model) -> History {
    let started = std::time::Instant::now();
    let mut dropped = 0;
    // undo: [oldest .. newest]. Patches (from an earlier reload) sit below the copies.
    let first_snap = undo.iter().position(|s| matches!(s, Step::Snap(_))).unwrap_or(undo.len());
    let mut out_undo: Vec<Step> = vec![];
    let mut converted: Vec<Patch> = vec![];
    let mut give_up = false;
    if first_snap < undo.len() {
        let mut after = serde_json::to_value(current).ok();
        for s in undo[first_snap..].iter().rev() {
            let Step::Snap(before) = s else { continue };
            if give_up || started.elapsed().as_secs() >= CONVERT_SECONDS {
                give_up = true;
                dropped += 1;
                continue;
            }
            let Some(a) = after.take() else {
                give_up = true;
                dropped += 1;
                continue;
            };
            let b = serde_json::to_value(before.as_ref()).unwrap_or(Value::Null);
            let p = Patch::between(&b, &a);
            after = Some(b);
            // a step that changed nothing in the model's content (a view, a selection) is no step
            if !p.is_empty() {
                converted.push(p);
            }
        }
    }
    if give_up {
        // a gap: what lies below it is let go too, rather than undone out of order
        dropped += first_snap;
    } else {
        for s in undo[..first_snap].iter() {
            if let Step::Patch(p) = s {
                out_undo.push(Step::Patch(Patch { parts: p.parts.iter().map(|(n, c)| (n.clone(), clone_change(c))).collect(), bytes: p.bytes }));
            }
        }
    }
    out_undo.extend(converted.into_iter().rev().map(Step::Patch));
    // redo: [bottom .. next]. Leading patches stay; copies are converted only when no patch is above them
    let mut out_redo: Vec<Step> = vec![];
    let mut patches_above = false;
    let mut chain: Vec<&Model> = vec![];
    for s in redo.iter().rev() {
        match s {
            Step::Patch(p) => {
                if chain.is_empty() {
                    patches_above = true;
                    out_redo.push(Step::Patch(Patch { parts: p.parts.iter().map(|(n, c)| (n.clone(), clone_change(c))).collect(), bytes: p.bytes }));
                } else {
                    dropped += 1;
                }
            }
            Step::Snap(m) => {
                if patches_above || started.elapsed().as_secs() >= CONVERT_SECONDS {
                    dropped += 1;
                } else {
                    chain.push(m.as_ref());
                }
            }
        }
    }
    if !chain.is_empty() {
        let mut prev = serde_json::to_value(current).unwrap_or(Value::Null);
        for m in chain {
            let next = serde_json::to_value(m).unwrap_or(Value::Null);
            let p = Patch::between(&prev, &next);
            prev = next;
            if !p.is_empty() {
                out_redo.push(Step::Patch(p));
            }
        }
    }
    // out_redo was built next-first; the list keeps the next step last
    out_redo.reverse();
    History { undo: out_undo, redo: out_redo, dropped }
}

fn clone_change(c: &Change) -> Change {
    match c {
        Change::Whole(a, b) => Change::Whole(a.clone(), b.clone()),
        Change::Keys(k) => Change::Keys(k.clone()),
    }
}
