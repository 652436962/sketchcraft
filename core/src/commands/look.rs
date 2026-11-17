//! How a model looks: materials, skins, rendering, environment and section planes.
//! (Arms of `Engine::run`; each runs on the model being edited: the open component's definition inside `edit`.)

use super::*;

impl Engine {
    pub(super) fn run_look(&mut self, cmd: Command) -> Result<Value> {
        let m: &mut Model = active_model!(self);
        Ok(match cmd {
            Command::Paint { ids, color, material } => match (material, color) {
                (Some(name), _) => {
                    m.paint_material(&ids, &name)?;
                    json!({"material": name})
                }
                (None, Some(c)) => json!({"material": m.paint(&ids, c)?}),
                _ => bail!("paint needs a color or a material"),
            },
            Command::Texture { name, data, width, height, color, project, origin } => {
                let mat = m.materials.entry(name.clone()).or_default();
                let mut tex = mat.texture.clone().unwrap_or(crate::model::Texture { data: String::new(), width: 1.0, height: 1.0, project: None, origin: None });
                if let Some(d) = data {
                    crate::safety::check_picture(&d).map_err(|e| anyhow::anyhow!("texture {name}: {e}"))?;
                    tex.data = d;
                }
                if tex.data.is_empty() {
                    bail!("a new texture needs image data");
                }
                if let Some(w) = width {
                    tex.height = height.unwrap_or(tex.height * w / tex.width.max(1e-9));
                    tex.width = w;
                } else if let Some(h) = height {
                    tex.height = h;
                }
                if let Some(p) = project {
                    tex.project = p;
                }
                if let Some(o) = origin {
                    tex.origin = Some(o);
                }
                if let Some(c) = color {
                    mat.color = c;
                } else if mat.color == [0.0; 3] {
                    mat.color = [0.8, 0.8, 0.8];
                }
                mat.texture = Some(tex);
                json!({"material": name})
            }
            Command::Photoreal { out, samples, bounces, mut sun_azimuth, mut sun_altitude, sun_strength, denoise, exposure, sky_strength, background, ground, ground_color, warmth, date, time, utc_offset, progressive, opts } => {
                let model = &self.model;
                let mut sun_note = None;
                if date.is_some() || time.is_some() {
                    let s = sun_at(model, date.as_deref(), time.as_deref(), utc_offset)?;
                    if s.altitude < 1.0 {
                        bail!("the sun is down then ({:.0}° below the horizon); pick a daytime hour", -s.altitude);
                    }
                    sun_azimuth = Some(s.azimuth);
                    sun_altitude = s.altitude;
                    sun_note = Some(serde_json::to_value(&s)?);
                }
                let camera = crate::pt::camera(model, &opts);
                // flat figures turn to face this camera
                let turned = model.facing(glam::DVec3::from(camera.eye));
                let model = turned.as_ref().unwrap_or(model);
                let po = crate::pt::PtOpts {
                    width: opts.width,
                    height: opts.height,
                    samples: samples.clamp(1, 4096),
                    bounces: bounces.clamp(1, 16),
                    camera: camera.clone(),
                    sun: crate::pt::sun_dir(
                        sun_azimuth.unwrap_or_else(|| {
                            // three-quarter light: over the camera's right shoulder, so the face you see is lit and shadows fall beside it
                            let (e, t) = (glam::DVec3::from(camera.eye), glam::DVec3::from(camera.target));
                            let back = e - t;
                            let az = back.x.atan2(back.y).to_degrees();
                            az + 45.0
                        }),
                        sun_altitude,
                    ),
                    sun_strength,
                    denoise,
                    exposure: exposure.clamp(0.01, 64.0),
                    sky_strength: sky_strength.clamp(0.0, 10.0),
                    background: background.as_deref().map(hex_rgb).transpose()?,
                    ground,
                    ground_color: ground_color.as_deref().map(hex_rgb).transpose()?,
                    warmth: warmth.clamp(-1.0, 1.0),
                }
                .limited();
                let t0 = std::time::Instant::now();
                let mut spp = po.samples;
                let img = if progressive {
                    // same picture as last time? keep adding to it
                    let key = format!("{}|{:?}|{:?}|{}|{}|{}|{}|{}", self.revision, po.camera.eye, po.camera.target, po.camera.fov, po.width, po.height, po.sun, po.bounces) + &format!("|{}|{}|{}|{:?}|{}|{:?}|{}", po.sun_strength, po.exposure, po.sky_strength, po.background, po.ground, po.ground_color, po.warmth);
                    let (p, img) = crate::pt::progressive(self.pt.take(), key, model, &po);
                    spp = p.spp;
                    self.pt = Some(p);
                    img
                } else {
                    crate::pt::render(model, &po)
                };
                let secs = t0.elapsed().as_secs_f64();
                let data = if let Some(path) = &out {
                    if let Some(dir) = std::path::Path::new(path).parent() {
                        std::fs::create_dir_all(dir).ok();
                    }
                    img.save(path)?;
                    None
                } else {
                    use base64::Engine as _;
                    let mut buf = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut buf, image::ImageFormat::Png)?;
                    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.into_inner())))
                };
                json!({"out": out, "data": data, "seconds": secs, "spp": spp, "sun": sun_note, "camera": {"eye": camera.eye, "target": camera.target, "fov": camera.fov}})
            }
            Command::Render { out, opts } => {
                let whole = &self.model;
                let cam = render::camera_for(whole, &opts);
                if let Some(path) = &out {
                    if let Some(dir) = std::path::Path::new(path).parent() {
                        std::fs::create_dir_all(dir).ok();
                    }
                    // flat figures turn to face this camera
                    let turned = whole.facing(glam::DVec3::from(cam.eye));
                    let fixed = render::RenderOpts { camera: Some(cam.clone()), ..opts.clone() };
                    render::render(turned.as_ref().unwrap_or(whole), &fixed).save(path)?;
                }
                json!({"out": out, "camera": {"eye": cam.eye, "target": cam.target, "fov": cam.fov}})
            }
            Command::Environment { .. } => bail!("environment runs through Engine::exec"),
            Command::Sun { date, time, utc_offset } => serde_json::to_value(sun_at(&self.model, date.as_deref(), time.as_deref(), utc_offset)?)?,
            Command::Section { origin, normal, path } => {
                let cut = m.section(v3(origin), v3(normal))?;
                if let Some(p) = &path {
                    let text = if p.to_ascii_lowercase().ends_with(".dxf") { cut.dxf() } else { cut.svg() };
                    std::fs::write(p, text)?;
                }
                // fills as triangles, ready to draw
                let mut fill: Vec<f64> = Vec::new();
                let mut owners: Vec<Id> = Vec::new();
                for (owner, o, h) in &cut.regions {
                    for t in crate::geom::triangulate(o, h, cut.n) {
                        fill.extend(t.iter().flat_map(|p| [p.x, p.y, p.z]));
                        owners.push(*owner);
                    }
                }
                let lines: Vec<f64> = cut.segments.iter().flat_map(|(a, b)| [a.x, a.y, a.z, b.x, b.y, b.z]).collect();
                json!({"regions": cut.regions.len(), "fill": fill, "fillOwners": owners, "lines": lines, "path": path})
            }
            Command::SectionPlane { name, origin, normal } => {
                let n = DVec3::from(normal);
                if !(n.length() > 1e-9) || !crate::model::finite3(&origin) {
                    bail!("a section plane needs a point and a direction");
                }
                let id = m.new_id();
                let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Section {}", m.sections.len() + 1));
                m.sections.push(crate::model::SectionPlane { id, name, origin, normal: n.normalize().to_array() });
                m.active_section = Some(id);
                json!({"section": id})
            }
            Command::SectionEdit { id, name, origin, normal, active, delete } => {
                let i = m.sections.iter().position(|s| s.id == id).ok_or_else(|| anyhow::anyhow!("no section plane {id}"))?;
                if delete {
                    m.sections.remove(i);
                    if m.active_section == Some(id) {
                        m.active_section = None;
                    }
                    return Ok(json!({"deleted": id}));
                }
                let s = &mut m.sections[i];
                if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
                    s.name = n.trim().chars().take(80).collect();
                }
                if let Some(o) = origin.filter(crate::model::finite3) {
                    s.origin = o;
                }
                if let Some(n) = normal.map(DVec3::from).filter(|n| n.length() > 1e-9) {
                    s.normal = n.normalize().to_array();
                }
                match active {
                    Some(true) => m.active_section = Some(id),
                    Some(false) if m.active_section == Some(id) => m.active_section = None,
                    _ => {}
                }
                json!({"section": id})
            }
            Command::SectionOff => {
                m.active_section = None;
                json!({})
            }
            Command::AddLocation { lat, lon, size, cells, imagery } => {
                let p = crate::geo::add_location(m, lat, lon, size, cells, imagery, &crate::geo::fetch)?;
                json!({"group": p.group, "elevation": p.elevation, "zoom": p.zoom})
            }
            Command::Thumbnail { component, asset, path, size } => {
                use base64::Engine as _;
                let px = size.clamp(32, 512);
                let png = if let Some(c) = component {
                    crate::library::thumbnail_of(m, c, px)?
                } else if let Some(a) = asset {
                    let mut scratch = Model::default();
                    let id = crate::assets::definition(&mut scratch, &a)?;
                    crate::library::thumbnail_of(&scratch, id, px)?
                } else if let Some(p) = path {
                    crate::library::thumbnail_of_file(&p, px)?
                } else {
                    bail!("thumbnail needs a component, asset or path");
                };
                json!({"data": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))})
            }
            Command::Purge => json!({"removed": crate::library::purge(m)}),
            Command::DeleteMaterial { name, replace_with } => {
                let faces = m.remove_material(&name, replace_with.as_deref())?;
                json!({"material": name, "faces": faces})
            }
            Command::PurgeMaterials => {
                let (names, bytes) = m.purge_materials();
                json!({"removed": names.len(), "materials": names, "freed_mb": bytes >> 20})
            }
            Command::Skins => {
                let dir = skins_dir();
                let mut out = Vec::new();
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sketchskin")).collect();
                    files.sort();
                    for p in files.into_iter().take(200) {
                        // JSON files only, up to 4 MB (a skin can carry an icon set)
                        if std::fs::metadata(&p).map(|m| m.len() > 4 << 20).unwrap_or(true) {
                            continue;
                        }
                        if let Some(skin) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                            out.push(json!({"path": p.to_string_lossy(), "name": p.file_stem().map(|s| s.to_string_lossy().to_string()), "skin": skin}));
                        }
                    }
                }
                json!({"dir": dir.to_string_lossy(), "skins": out})
            }
            Command::SaveSkin { name, skin } => {
                let dir = skins_dir();
                std::fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{}.sketchskin", crate::library::safe(&name)));
                std::fs::write(&path, serde_json::to_string_pretty(&skin)?)?;
                json!({"path": path.to_string_lossy()})
            }
            Command::Material { name, color, roughness, metalness, glass, emission } => {
                let mat = m.materials.entry(name.clone()).or_default();
                if let Some(c) = color {
                    mat.color = c;
                }
                let unit = |x: f32| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.5 };
                if let Some(r) = roughness {
                    mat.roughness = Some(unit(r));
                }
                if let Some(x) = metalness {
                    mat.metalness = Some(unit(x));
                }
                if let Some(g) = glass {
                    mat.glass = Some(g);
                }
                if let Some(e) = emission {
                    mat.emission = Some(if e.is_finite() { e.clamp(0.0, 100.0) } else { 0.0 });
                }
                json!({"material": name})
            }
            Command::SaveImage { path, data } => {
                use base64::Engine as _;
                let b64 = data.strip_prefix("data:image/png;base64,").ok_or_else(|| anyhow::anyhow!("expected a PNG data URL"))?;
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
                // only a real picture is written: decode it (this also bounds its size)
                let img = crate::safety::decode_image(&bytes, Some(image::ImageFormat::Png)).map_err(|e| anyhow::anyhow!("that isn't a PNG image the program can save: {e}"))?;
                let _ = img;
                if let Some(dir) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(dir).ok();
                }
                crate::safety::write_atomic(std::path::Path::new(&path), &bytes)?;
                json!({"path": path})
            }
            other => unreachable!("command routed to the wrong group: {other:?}"),
        })
    }
}
