//! Groups, components, tags, visibility, selection by query and the other ways a model is organised.
//! (Arms of `Engine::run`; each runs on the model being edited: the open component's definition inside `edit`.)

use super::*;

impl Engine {
    pub(super) fn run_structure(&mut self, cmd: Command) -> Result<Value> {
        let m: &mut Model = active_model!(self);
        Ok(match cmd {
            Command::BeginGroup { name } => {
                let n = name.unwrap_or_else(|| format!("Group {}", m.groups.len() + 1));
                json!({"group": m.begin_group(n)})
            }
            Command::EndGroup => {
                // back out one level (to the group this one sits in)
                m.active_group = m.active_group.and_then(|g| m.groups.get(&g)).and_then(|g| g.parent);
                json!({})
            }
            Command::Group { ids, name } => {
                let n = name.unwrap_or_else(|| format!("Group {}", m.groups.len() + 1));
                json!({"group": m.group(&ids, n)?})
            }
            Command::Faces { group, facing } => {
                let dir = facing.map(|f| v3(f).normalize_or_zero());
                let list: Vec<Value> = m
                    .faces
                    .iter()
                    .filter(|(_, f)| group.is_none() || f.group == group)
                    .map(|(&id, _)| id)
                    .filter(|&id| dir.map_or(true, |d| m.normal(id).dot(d) > 0.94))
                    .map(|id| {
                        let f = &m.faces[&id];
                        let r = |v: DVec3| [round(v.x), round(v.y), round(v.z)];
                        json!({
                            "id": id,
                            "normal": r(m.normal(id)),
                            "center": r(m.center(id)),
                            "area": round(m.face_area(id)),
                            "points": f.outer.len(),
                            "holes": f.holes.len(),
                            "group": f.group,
                            "material": f.material,
                        })
                    })
                    .collect();
                json!({"faces": list})
            }
            Command::Pick { at, facing } => {
                let p = v3(at);
                let dir = facing.map(|f| v3(f).normalize_or_zero());
                let candidates: Vec<Id> = m
                    .faces
                    .keys()
                    .copied()
                    .filter(|&id| dir.map_or(true, |d| m.normal(id).dot(d) > 0.94))
                    .collect();
                let on = candidates.iter().copied().find(|&id| m.contains(id, p));
                let face = on.or_else(|| {
                    candidates.iter().copied().min_by(|a, b| {
                        m.center(*a).distance(p).partial_cmp(&m.center(*b).distance(p)).unwrap()
                    })
                });
                let Some(face) = face else { bail!("no face there") };
                json!({"face": face, "exact": on.is_some()})
            }
            Command::Copy { group, by, name } => json!({"group": m.copy_group(group, v3(by), name)?}),
            Command::MakeComponent { ids, name } => {
                let n = name.unwrap_or_else(|| format!("Component {}", m.definitions.len() + 1));
                let inst = m.make_component(&ids, n)?;
                json!({"instance": inst, "component": m.instances[&inst].def})
            }
            Command::Place { component, at, rotate, name, host } => {
                let key = match &component {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let Some(def) = m.find_definition(&key) else { bail!("no component {key}") };
                let instance = m.place(def, v3(at), rotate, name)?;
                let opening = place_cut(m, instance, host);
                json!({"instance": instance, "opening": opening})
            }
            Command::ComponentFaceMe { id, on } => {
                let def = m.instances.get(&id).map(|i| i.def).unwrap_or(id);
                let d = m.definitions.get_mut(&def).ok_or_else(|| anyhow::anyhow!("{id} is not a component"))?;
                d.face_me = on;
                json!({"component": def, "face_me": on})
            }
            Command::ComponentCuts { id, cuts } => {
                let def = m.instances.get(&id).map(|i| i.def).unwrap_or(id);
                let d = m.definitions.get_mut(&def).ok_or_else(|| anyhow::anyhow!("{id} is not a component"))?;
                d.cuts = cuts;
                json!({"component": def, "cuts": cuts})
            }
            Command::Edit { id } => {
                if m.groups.contains_key(&id) {
                    m.active_group = Some(id); // new geometry and lines go inside the group
                    return Ok(json!({"editing_group": id}));
                }
                let Some(i) = self.model.instances.get(&id) else { bail!("{id} is not a group or component placement") };
                self.ctx = Some(i.def);
                json!({"editing": i.def})
            }
            Command::Close => {
                // out of a component; or out of a group, one level at a time
                if self.ctx.is_some() {
                    self.ctx = None;
                } else {
                    m.active_group = m.active_group.and_then(|g| m.groups.get(&g)).and_then(|g| g.parent);
                }
                json!({})
            }
            Command::Explode { id } => json!({"group": m.explode(id)?}),
            Command::Components => json!({"components": m.definitions.iter().map(|(id, d)| json!({
                "id": id, "name": d.name, "faces": d.model.faces.len(),
                "placed": m.instances.values().filter(|i| i.def == *id).count(),
            })).collect::<Vec<_>>()}),
            Command::Hide { ids } => {
                m.set_hidden(&ids, true)?;
                json!({})
            }
            Command::Unhide { ids } => {
                if ids.is_empty() { m.unhide_all() } else { m.set_hidden(&ids, false)? }
                json!({})
            }
            Command::Tag { ids, tag } => {
                m.set_tag(&ids, tag)?;
                json!({})
            }
            Command::TagVisible { tag, visible } => {
                m.tags.insert(tag, visible);
                json!({})
            }
            Command::TagColor { tag, color } => {
                match color {
                    Some(c) => m.tag_colors.insert(tag, c),
                    None => m.tag_colors.remove(&tag),
                };
                json!({})
            }
            Command::RenameTag { tag, to } => {
                let to = to.trim().to_string();
                if to.is_empty() {
                    bail!("a tag needs a name");
                }
                if let Some(v) = m.tags.remove(&tag) {
                    m.tags.insert(to.clone(), v);
                }
                if let Some(c) = m.tag_colors.remove(&tag) {
                    m.tag_colors.insert(to.clone(), c);
                }
                for g in m.groups.values_mut().filter(|g| g.tag.as_deref() == Some(tag.as_str())) {
                    g.tag = Some(to.clone());
                }
                for i in m.instances.values_mut().filter(|i| i.tag.as_deref() == Some(tag.as_str())) {
                    i.tag = Some(to.clone());
                }
                json!({"tag": to})
            }
            Command::SetParent { ids, parent } => {
                m.set_parent(&ids, parent)?;
                json!({})
            }
            Command::Ungroup { id } => {
                m.check_unlocked(&[id])?;
                m.ungroup(id)?;
                json!({})
            }
            Command::Lock { ids, locked } => {
                for id in &ids {
                    let Some(g) = m.groups.get_mut(id) else { bail!("{id} is not a group") };
                    g.locked = locked;
                }
                json!({})
            }
            Command::Soften { ids, soft, coplanar } => {
                let n = if coplanar { m.soften_coplanar() } else { m.soften(&ids, soft)? };
                json!({"edges": n})
            }
            Command::Rename { id, name } => {
                if let Some(g) = m.groups.get_mut(&id) {
                    g.name = name;
                } else if let Some(i) = m.instances.get_mut(&id) {
                    i.name = name;
                } else if let Some(d) = m.definitions.get_mut(&id) {
                    d.name = name;
                } else {
                    bail!("{id} can't be renamed (groups and components only)");
                }
                json!({})
            }
            Command::MakeUnique { id } => json!({"component": m.make_unique(id)?}),
            Command::Opacity { material, value } => {
                let Some(mat) = m.materials.get_mut(&material) else { bail!("no material {material}") };
                mat.opacity = value.clamp(0.05, 1.0);
                json!({"opacity": mat.opacity})
            }
            Command::Connected { id, all } => {
                let (faces, edges) = m.connected(id, all)?;
                json!({"faces": faces, "edges": edges})
            }
            Command::SameAs { material, tag, by } => {
                let ctx = m.active_group;
                if by == "tag" {
                    let groups: Vec<Id> = m.groups.iter().filter(|(_, g)| g.parent == ctx && g.tag == tag).map(|(k, _)| *k).collect();
                    let instances: Vec<Id> = m.instances.iter().filter(|(_, i)| i.parent == ctx && i.tag == tag).map(|(k, _)| *k).collect();
                    json!({"groups": groups, "instances": instances})
                } else {
                    let faces: Vec<Id> = m.faces.iter().filter(|(_, f)| f.group == ctx && f.material == material && !f.hidden).map(|(k, _)| *k).collect();
                    json!({"faces": faces})
                }
            }
            Command::Resize { factor } => {
                m.resize(factor)?;
                json!({"factor": factor})
            }
            Command::SetAxes { origin, x, y, reset } => {
                if reset {
                    m.axes = None;
                    return Ok(json!({"axes": null}));
                }
                let (Some(o), Some(x), Some(y)) = (origin, x, y) else { bail!("set_axes needs origin, x and y (or reset)") };
                let x = DVec3::from(x).normalize_or_zero();
                let y = DVec3::from(y);
                let y = (y - x * y.dot(x)).normalize_or_zero();
                if x == DVec3::ZERO || y == DVec3::ZERO || !crate::model::finite3(&o) {
                    bail!("the red and green directions need to be different");
                }
                m.axes = Some([o, x.to_array(), y.to_array()]);
                json!({"axes": m.axes})
            }
            Command::SetAttrs { id, attrs } => {
                let target = if let Some(i) = m.instances.get_mut(&id) {
                    &mut i.attrs
                } else if let Some(d) = m.definitions.get_mut(&id) {
                    &mut d.attrs
                } else {
                    bail!("{id} is not a component placement or a component");
                };
                for (k, v) in attrs {
                    let k = k.trim().to_ascii_lowercase();
                    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        bail!("attribute names are letters, digits and _ ({k:?})");
                    }
                    if v.trim().is_empty() {
                        target.remove(&k);
                    } else {
                        target.insert(k, v.trim().to_string());
                    }
                }
                // check every formula still works out
                let (inst, def) = match m.instances.get(&id) {
                    Some(i) => (Some(i.clone()), m.definitions.get(&i.def)),
                    None => (None, m.definitions.get(&id)),
                };
                let all = match &inst {
                    Some(i) => crate::dynamic::attrs_of(i, def),
                    None => def.map(|d| d.attrs.clone()).unwrap_or_default(),
                };
                let vals = crate::dynamic::resolve(&all, &Default::default());
                for (k, v) in &all {
                    if v.starts_with('=') && !vals.contains_key(k) && !v.contains("parent!") {
                        let err = crate::dynamic::eval(v, &|n| vals.get(n).copied().ok_or_else(|| anyhow::anyhow!("no attribute {n}"))).err();
                        bail!("{k}: {}", err.map(|e| e.to_string()).unwrap_or_else(|| "can't be worked out".into()));
                    }
                }
                json!({"attrs": all, "values": vals})
            }
            Command::Classify { ids, class } => {
                let class = class.trim().to_string();
                if !class.is_empty() && !crate::ifc::CLASSES.iter().any(|c| c.eq_ignore_ascii_case(&class)) {
                    bail!("unknown IFC type {class:?}; one of {}", crate::ifc::CLASSES.join(", "));
                }
                let class = crate::ifc::CLASSES.iter().find(|c| c.eq_ignore_ascii_case(&class)).map(|c| c.to_string());
                for id in &ids {
                    if !m.groups.contains_key(id) && !m.instances.contains_key(id) {
                        bail!("{id} is not a group or component");
                    }
                    match &class {
                        Some(c) => m.classes.insert(*id, c.clone()),
                        None => m.classes.remove(id),
                    };
                }
                json!({"classified": ids.len()})
            }
            Command::Reveal { path } => {
                let p = crate::safety::path_arg(path.map(std::path::PathBuf::from).unwrap_or_else(crate::library::default_dir));
                std::fs::create_dir_all(if p.extension().is_some() { p.parent().unwrap_or(&p) } else { &p }).ok();
                // folders open; files and app bundles are only shown, never launched
                #[cfg(target_os = "macos")]
                let r = {
                    let bundle = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
                    if p.is_dir() && !bundle { std::process::Command::new("open").arg(&p).status() } else { std::process::Command::new("open").arg("-R").arg(&p).status() }
                };
                // never `explorer <file>`: that opens (runs) the file. Folders open; a file is only selected.
                #[cfg(target_os = "windows")]
                let r = if p.is_dir() { std::process::Command::new("explorer").arg(&p).status() } else { std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).status() };
                #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
                let r = std::process::Command::new("xdg-open").arg(if p.is_dir() { &p } else { p.parent().unwrap_or(&p) }).status();
                anyhow::Context::context(r, "couldn't open the file browser")?;
                json!({"path": p.to_string_lossy()})
            }
            other => unreachable!("command routed to the wrong group: {other:?}"),
        })
    }
}
