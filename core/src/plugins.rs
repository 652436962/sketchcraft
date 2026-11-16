//! Plugins: small scripts that add commands to SketchCraft. Each is a folder
//! with `plugin.json` (its name and commands) and `main.js`, run by the app in
//! a sandbox (a Web Worker with no page, network or file access) that can
//! only call engine commands. Built-in plugins ship inside the app; others
//! live in ~/Documents/SketchCraft/Plugins. Also here: `parts`, the sizes a
//! cut list needs.

use crate::model::{Id, Model};
use glam::DVec3;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

const BUILTIN: &[(&str, &str, &str)] = &[("cutlist", include_str!("../plugins/cutlist/plugin.json"), include_str!("../plugins/cutlist/main.js"))];

/// Where people put their own plugins.
pub fn dir() -> PathBuf {
    crate::live::default_doc().parent().map(|p| p.join("Plugins")).unwrap_or_else(|| PathBuf::from("Plugins"))
}

fn entry(id: &str, manifest: &str, code: String, builtin: bool) -> Option<Value> {
    let mut m: Value = serde_json::from_str(manifest).ok()?;
    let o = m.as_object_mut()?;
    o.insert("id".into(), json!(id));
    o.insert("code".into(), json!(code));
    o.insert("builtin".into(), json!(builtin));
    Some(m)
}

/// Every plugin: manifest fields, its code, and whether it's built in. A
/// folder that can't be read is listed in `problems` instead.
pub fn list() -> Value {
    let mut out: Vec<Value> = BUILTIN.iter().filter_map(|(id, m, c)| entry(id, m, c.to_string(), true)).collect();
    let mut problems = Vec::new();
    let d = dir();
    std::fs::create_dir_all(&d).ok(); // so "Open plugins folder" has somewhere to go
    if let Ok(rd) = std::fs::read_dir(&d) {
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for p in dirs {
            let id = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let manifest = std::fs::read_to_string(p.join("plugin.json"));
            let code = std::fs::read_to_string(p.join("main.js"));
            match (manifest, code) {
                (Ok(m), Ok(c)) if c.len() <= 1 << 20 => match entry(&id, &m, c, false) {
                    Some(v) => out.push(v),
                    None => problems.push(json!({"id": id, "error": "plugin.json isn't valid JSON"})),
                },
                (Ok(_), Ok(_)) => problems.push(json!({"id": id, "error": "main.js is over 1 MB"})),
                _ => problems.push(json!({"id": id, "error": "needs plugin.json and main.js"})),
            }
        }
    }
    json!({"dir": d, "plugins": out, "problems": problems})
}

/// A part: name, kind, material, and size as [length, width, thickness]
/// (largest first), measured along its biggest face.
pub fn parts(m: &Model, ids: &[Id]) -> Vec<Value> {
    let wanted: Vec<Id> = if ids.is_empty() {
        // every group with nothing grouped inside it, and every component
        let mut v: Vec<Id> = m.groups.iter().filter(|(id, g)| !g.hidden && !m.groups.values().any(|o| o.parent == Some(**id))).map(|(id, _)| *id).collect();
        v.extend(m.instances.iter().filter(|(_, i)| !i.hidden).map(|(id, _)| *id));
        v
    } else {
        ids.to_vec()
    };
    let world = m.world_faces();
    wanted
        .into_iter()
        .filter_map(|id| {
            let (name, kind, faces): (String, &str, Vec<_>) = if let Some(g) = m.groups.get(&id) {
                let tree = m.subtree(id);
                let mine = |f: &&crate::components::WFace| f.face.and_then(|fid| m.faces.get(&fid)).and_then(|x| x.group).is_some_and(|x| tree.contains(&x));
                (g.name.clone(), "group", world.iter().filter(mine).map(|f| (f.outer.clone(), f.normal, f.material.clone(), f.color)).collect())
            } else if let Some(i) = m.instances.get(&id) {
                (i.name.clone(), "component", world.iter().filter(|f| f.owner == id).map(|f| (f.outer.clone(), f.normal, f.material.clone(), f.color)).collect())
            } else {
                return None;
            };
            if faces.is_empty() {
                return None;
            }
            // axes: the biggest face's normal and its longest edge
            let area = |pts: &Vec<DVec3>, n: DVec3| -> f64 {
                let mut s = DVec3::ZERO;
                for k in 0..pts.len() {
                    s += pts[k].cross(pts[(k + 1) % pts.len()]);
                }
                (s.dot(n) * 0.5).abs()
            };
            let big = faces.iter().max_by(|a, b| area(&a.0, a.1).total_cmp(&area(&b.0, b.1)))?;
            let n = big.1.normalize_or_zero();
            let mut u = DVec3::ZERO;
            for k in 0..big.0.len() {
                let e = big.0[(k + 1) % big.0.len()] - big.0[k];
                let e = e - n * e.dot(n);
                if e.length() > u.length() {
                    u = e;
                }
            }
            let u = u.normalize_or_zero();
            let w = n.cross(u).normalize_or_zero();
            if u == DVec3::ZERO || w == DVec3::ZERO {
                return None;
            }
            let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
            for (pts, ..) in &faces {
                for p in pts {
                    let q = DVec3::new(p.dot(u), p.dot(w), p.dot(n));
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
            }
            let mut size = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
            size.sort_by(|a, b| b.total_cmp(a));
            // the material most of its faces wear
            let mut count: BTreeMap<String, usize> = BTreeMap::new();
            for (.., mat, col) in &faces {
                let key = mat.clone().or_else(|| col.map(|c| format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8))).unwrap_or_default();
                *count.entry(key).or_default() += 1;
            }
            let material = count.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k).filter(|k| !k.is_empty());
            Some(json!({"id": id, "name": name, "kind": kind, "material": material, "size": size.map(|x| (x * 10000.0).round() / 10000.0)}))
        })
        .collect()
}
