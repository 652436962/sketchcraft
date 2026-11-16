//! Plugins: small scripts that add commands to SketchCraft. Each is a folder
//! with `plugin.json` (its name and commands) and `main.js`, run by the app in
//! a sandbox (a Web Worker with no page, network or file access) that can
//! only call engine commands. Built-in plugins ship inside the app; others
//! live in ~/Documents/SketchCraft/Plugins. Also here: `parts`, the sizes a
//! cut list needs.

use crate::model::{Id, Model};
use glam::DVec3;
use serde_json::{json, Value};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const BUILTIN: &[(&str, &str, &str)] = &[
    ("cutlist", include_str!("../plugins/cutlist/plugin.json"), include_str!("../plugins/cutlist/main.js")),
    ("shelves", include_str!("../plugins/shelves/plugin.json"), include_str!("../plugins/shelves/main.js")),
    ("framing", include_str!("../plugins/framing/plugin.json"), include_str!("../plugins/framing/main.js")),
];

/// Where people put their own plugins.
pub fn dir() -> PathBuf {
    crate::live::default_doc().parent().map(|p| p.join("Plugins")).unwrap_or_else(|| PathBuf::from("Plugins"))
}

fn entry(id: &str, manifest: &str, code: String, builtin: bool, folder: Option<&Path>) -> Option<Value> {
    let mut m: Value = serde_json::from_str(manifest).ok()?;
    let o = m.as_object_mut()?;
    o.insert("id".into(), json!(id));
    o.insert("code".into(), json!(code));
    o.insert("builtin".into(), json!(builtin));
    o.insert("enabled".into(), json!(!disabled().contains(id)));
    if let Some(f) = folder {
        o.insert("folder".into(), json!(f));
    }
    // command icons, sent as data URLs: inline SVG ("<svg ...>"), or a small
    // SVG or PNG file beside main.js
    if let Some(cmds) = o.get_mut("commands").and_then(|c| c.as_array_mut()) {
        for c in cmds {
            let Some(icon) = c.get("icon").and_then(|i| i.as_str()).map(str::to_string) else { continue };
            let data = if icon.trim_start().starts_with("<svg") && icon.len() <= 64 * 1024 {
                use base64::Engine as _;
                Some(format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(icon.trim())))
            } else {
                folder.and_then(|f| icon_data(f, &icon))
            };
            c.as_object_mut()?.insert("icon".into(), data.map(Value::from).unwrap_or(Value::Null));
        }
    }
    Some(m)
}

/// A plugin's icon file as a data URL (SVG or PNG, up to 256 KB, inside its folder).
fn icon_data(folder: &Path, name: &str) -> Option<String> {
    use base64::Engine as _;
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return None;
    }
    let kind = match Path::new(name).extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => return None,
    };
    let bytes = std::fs::read(folder.join(name)).ok().filter(|b| b.len() <= 256 * 1024)?;
    Some(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Plugins turned off (by id), kept in the Plugins folder.
fn disabled() -> BTreeSet<String> {
    std::fs::read_to_string(dir().join("disabled.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn enable(id: &str, on: bool) -> Result<Value> {
    let mut d = disabled();
    if on {
        d.remove(id);
    } else {
        d.insert(id.to_string());
    }
    std::fs::create_dir_all(dir())?;
    std::fs::write(dir().join("disabled.json"), serde_json::to_string_pretty(&d)?)?;
    Ok(json!({"id": id, "enabled": on}))
}

/// A folder name for a plugin: lowercase letters, digits and dashes.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(40).collect()
}

const MAX_ZIP: usize = 8 << 20;
const MAX_FILES: usize = 64;
const MAX_UNPACKED: u64 = 16 << 20;

/// Install a plugin from a .zip's bytes: plugin.json and main.js at its top,
/// or in one folder inside. Only plain files with safe names are unpacked
/// (no paths out of the folder, no hidden files, size limits). Replaces an
/// older copy of the same plugin, which is kept in .removed.
pub fn install(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > MAX_ZIP {
        bail!("that plugin is over 8 MB");
    }
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("that isn't a .zip file")?;
    if z.len() > MAX_FILES {
        bail!("that plugin has over {MAX_FILES} files");
    }
    // where plugin.json is: the top, or one folder down
    let mut names = Vec::new();
    for i in 0..z.len() {
        names.push(z.by_index(i)?.name()?.to_string());
    }
    let manifest_at = names.iter().find(|n| n.trim_start_matches("./") == "plugin.json" || (n.ends_with("/plugin.json") && n.matches('/').count() == 1)).cloned().ok_or_else(|| anyhow::anyhow!("no plugin.json in it"))?;
    let prefix = manifest_at.trim_start_matches("./").trim_end_matches("plugin.json").to_string();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name()?.trim_start_matches("./").to_string();
        let Some(rel) = name.strip_prefix(&prefix) else { continue };
        // safe names only: no parent dirs, no absolute paths, no hidden files
        if rel.is_empty() || rel.split('/').any(|p| p.is_empty() || p == ".." || p.starts_with('.')) || rel.contains('\\') || rel.contains(':') {
            bail!("unsafe file name in the plugin: {name}");
        }
        let ext = Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !matches!(ext.as_str(), "json" | "js" | "svg" | "png" | "md" | "txt") {
            continue; // other files aren't needed
        }
        total += f.size();
        if total > MAX_UNPACKED {
            bail!("that plugin unpacks to over 16 MB");
        }
        let mut b = Vec::new();
        f.by_ref().take(MAX_UNPACKED).read_to_end(&mut b)?;
        files.push((rel.to_string(), b));
    }
    let manifest = files.iter().find(|(n, _)| n == "plugin.json").map(|(_, b)| b.clone()).ok_or_else(|| anyhow::anyhow!("no plugin.json"))?;
    if !files.iter().any(|(n, _)| n == "main.js") {
        bail!("no main.js next to plugin.json");
    }
    let m: Value = serde_json::from_slice(&manifest).context("plugin.json isn't valid JSON")?;
    let name = m.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()).ok_or_else(|| anyhow::anyhow!("plugin.json needs a name"))?;
    if !m.get("commands").is_some_and(|c| c.is_array()) {
        bail!("plugin.json needs a commands list");
    }
    let id = slug(name);
    if id.is_empty() || BUILTIN.iter().any(|(b, ..)| *b == id) {
        bail!("pick another name for this plugin");
    }
    let root = dir();
    std::fs::create_dir_all(&root)?;
    let target = root.join(&id);
    let replaced = target.exists();
    if replaced {
        set_aside(&target, &id)?;
    }
    for (rel, b) in &files {
        let p = target.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, b)?;
    }
    // installing it means you want it: on, even if an older copy was off
    if disabled().contains(&id) {
        enable(&id, true)?;
    }
    Ok(json!({"id": id, "name": name, "files": files.len(), "replaced": replaced}))
}

fn set_aside(folder: &Path, id: &str) -> Result<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // a fresh name, even twice in one second
    let mut away = dir().join(".removed").join(format!("{id}-{stamp}"));
    let mut n = 1;
    while away.exists() {
        n += 1;
        away = dir().join(".removed").join(format!("{id}-{stamp}-{n}"));
    }
    std::fs::create_dir_all(away.parent().unwrap())?;
    std::fs::rename(folder, &away)?;
    Ok(away)
}

/// Move one of your plugins aside (to Plugins/.removed), so it can be put back.
pub fn remove(id: &str) -> Result<Value> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.starts_with('.') || BUILTIN.iter().any(|(b, ..)| *b == id) {
        bail!("can't remove {id:?}");
    }
    let folder = dir().join(id);
    if !folder.is_dir() {
        bail!("no plugin {id}");
    }
    let away = set_aside(&folder, id)?;
    Ok(json!({"id": id, "moved_to": away}))
}

/// Every plugin: manifest fields, its code, and whether it's built in. A
/// folder that can't be read is listed in `problems` instead.
pub fn list() -> Value {
    let mut out: Vec<Value> = BUILTIN.iter().filter_map(|(id, m, c)| entry(id, m, c.to_string(), true, None)).collect();
    let mut problems = Vec::new();
    let d = dir();
    std::fs::create_dir_all(&d).ok(); // so "Open plugins folder" has somewhere to go
    if let Ok(rd) = std::fs::read_dir(&d) {
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        dirs.sort();
        for p in dirs {
            let id = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let manifest = std::fs::read_to_string(p.join("plugin.json"));
            let code = std::fs::read_to_string(p.join("main.js"));
            match (manifest, code) {
                (Ok(m), Ok(c)) if c.len() <= 1 << 20 => match entry(&id, &m, c, false, Some(&p)) {
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
        // components, but not scale figures (flat people aren't parts)
        v.extend(m.instances.iter().filter(|(_, i)| !i.hidden && !m.definitions.get(&i.def).is_some_and(|d| d.face_me)).map(|(id, _)| *id));
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
            if size[2] < 1e-4 {
                return None; // flat: a drawing or a figure, not something to cut
            }
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
