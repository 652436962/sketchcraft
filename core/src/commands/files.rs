//! Files in and out: save/open, exports, imports, the library, plugins, animation and video.
//! (Arms of `Engine::run`; each runs on the model being edited: the open component's definition inside `edit`.)

use super::*;

impl Engine {
    pub(super) fn run_files(&mut self, cmd: Command) -> Result<Value> {
        let m: &mut Model = active_model!(self);
        Ok(match cmd {
            Command::Plugins => crate::plugins::list(),
            Command::PluginEnable { id, on } => crate::plugins::enable(&id, on)?,
            Command::InstallPlugin { path, data, name, preview, replace } => {
                let name = name.or_else(|| path.clone());
                let bytes = match (path, data) {
                    (Some(p), _) => std::fs::read(&p)?,
                    (None, Some(d)) => {
                        use base64::Engine as _;
                        base64::engine::general_purpose::STANDARD.decode(d.trim().trim_start_matches("data:application/zip;base64,"))?
                    }
                    _ => bail!("install_plugin needs a path or data"),
                };
                crate::plugins::install(&bytes, name.as_deref(), &crate::plugins::InstallOpts { preview, replace })?
            }
            Command::RemovePlugin { id } => crate::plugins::remove(&id)?,
            Command::SaveText { path, text } => {
                crate::safety::write_atomic(&path, text.as_bytes())?;
                json!({"path": path})
            }
            Command::Save { path } => {
                // never write a model with NaN or absurd numbers: it couldn't be opened again
                self.model.check_sane()?;
                crate::safety::write_atomic(&path, serde_json::to_string(&self.model)?.as_bytes())?;
                json!({"path": path})
            }
            Command::Open { path } => {
                // the whole file is read into memory (a model takes several times its file size once opened): look at the size first
                let len = std::fs::metadata(&path)?.len();
                if len > MAX_OPEN_BYTES {
                    bail!("{path} is {} MB; SketchCraft opens models up to {} MB", len >> 20, MAX_OPEN_BYTES >> 20);
                }
                // an agent opens what an agent may hold (its own model cap, with room for the file's escapes), not the person's whole 768 MB
                if self.untrusted && len > MAX_OPEN_BYTES_UNTRUSTED {
                    bail!("{path} is {} MB; an agent or a plugin can open models up to {} MB (the person can open larger ones)", len >> 20, MAX_OPEN_BYTES_UNTRUSTED >> 20);
                }
                let mut opened: Model = serde_json::from_str(&std::fs::read_to_string(&path)?).map_err(|e| anyhow::anyhow!("{path} isn't a model SketchCraft can read: {e}"))?;
                if let Some(what) = opened.broken_refs(false).first() {
                    bail!("{path} isn't a consistent model ({what} refers to something that isn't there)");
                }
                // ids from outside: the counter must be above every id in the file, or the next drawing overwrites something
                opened.repair_ids();
                opened.vet_pictures();
                if self.untrusted && opened.size_estimate() > crate::model::MAX_MODEL_BYTES_UNTRUSTED {
                    bail!("{path} is a model of about {} MB; an agent or a plugin can open models up to {} MB (the person can open larger ones)", opened.size_estimate() >> 20, crate::model::MAX_MODEL_BYTES_UNTRUSTED >> 20);
                }
                // too much geometry or too many bytes is not a reason to refuse the file: it opens for viewing and cutting down (see `oversize`)
                opened.check_sane_with(false)?;
                self.oversize = opened.over_limits();
                self.model = opened;
                self.batch = None;
                self.ctx = None;
                self.names.clear();
                json!({"path": path, "faces": self.model.faces.len()})
            }
            Command::ExportGlb { path } => json!({"path": path, "triangles": export::write_glb(&self.model, &path)?}),
            Command::ExportStl { path, scale } => json!({"path": path, "triangles": export::write_stl(&self.model, &path, scale)?}),
            Command::Animate { path, fps, transition, delay, views, turntable, timeline, opts } => {
                let fps = fps.clamp(1.0, 60.0);
                if timeline {
                    let model = &self.model;
                    let fs = crate::timeline::frames(model, &model.timeline, fps, &|d, t| timeline_sun(model, d, t))?;
                    if fs.is_empty() {
                        bail!("the timeline is empty: add a clip first");
                    }
                    return Ok(json!({"path": path, "frames": crate::animate::animate_cams(model, &path, &crate::timeline::cameras(&fs), fps, &opts)?}));
                }
                let a = crate::animate::Anim { fps, transition, delay, views, turntable };
                json!({"path": path, "frames": crate::animate::animate(&self.model, &path, &a, &opts)?})
            }
            Command::Blueprint { path, sheet, project, notes } => {
                let project = project.unwrap_or_else(|| std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Project".into()));
                let building = crate::environment::without(&self.model);
                json!({"path": path, "pages": crate::layout::blueprint(&building, &path, &sheet, &project, &notes)?, "levels": crate::layout::floor_levels(&building)})
            }
            Command::Levels => json!({"levels": crate::layout::floor_levels(&crate::environment::without(&self.model))}),
            Command::Timeline { clips } => {
                m.timeline = crate::timeline::Timeline { clips };
                // check it now, so a bad clip is caught when it's made
                let model = &*m;
                let n = crate::timeline::frames(model, &model.timeline, 10.0, &|d, t| timeline_sun(model, d, t))?.len();
                json!({"clips": m.timeline.clips.len(), "seconds": m.timeline.clips.iter().map(|c| c.seconds).sum::<f64>(), "frames_at_10fps": n})
            }
            Command::TimelineFrames { fps } => {
                let fps = fps.clamp(1.0, 60.0);
                let model = &self.model;
                let fs = crate::timeline::frames(model, &model.timeline, fps, &|d, t| timeline_sun(model, d, t))?;
                json!({"fps": fps, "frames": fs})
            }
            Command::VideoFrame { session, index, data } => {
                use base64::Engine as _;
                if index >= crate::timeline::MAX_FRAMES || data.len() > 40 << 20 {
                    bail!("that frame is out of range");
                }
                let dir = video_dir(&session)?;
                std::fs::create_dir_all(&dir)?;
                let (head, b64) = data.split_once(',').unwrap_or(("", &data));
                let ext = if head.contains("jpeg") { "jpg" } else { "png" };
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
                std::fs::write(dir.join(format!("frame_{index:05}.{ext}")), bytes)?;
                json!({"index": index})
            }
            Command::VideoFinish { session, path, fps } => {
                let dir = video_dir(&session)?;
                let ext = if dir.join("frame_00000.jpg").exists() { "jpg" } else { "png" };
                let out = std::process::Command::new("ffmpeg")
                    .args(["-y", "-loglevel", "error", "-framerate"])
                    .arg(format!("{}", fps.clamp(1.0, 60.0)))
                    .arg("-i")
                    .arg(dir.join(format!("frame_%05d.{ext}")))
                    .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"])
                    .arg(crate::safety::path_arg(&path))
                    .output();
                let frames = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
                std::fs::remove_dir_all(&dir).ok();
                match out {
                    Ok(o) if o.status.success() => json!({"path": path, "frames": frames}),
                    Ok(o) => bail!("ffmpeg couldn't make the video: {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("")),
                    Err(_) => bail!("making a video needs ffmpeg installed (brew install ffmpeg)"),
                }
            }
            Command::ImportDxf { path, text, units, at, name } => {
                let text = match (text, &path) {
                    (Some(t), _) => t,
                    (None, Some(p)) => String::from_utf8_lossy(&std::fs::read(p)?).into_owned(),
                    _ => bail!("import_dxf needs a path or text"),
                };
                let label = name.or_else(|| path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()).map(|s| tidy_name(&s.to_string_lossy()))).unwrap_or_else(|| "DXF drawing".into());
                let r = crate::dxf::import(m, &text, &label, units.as_deref(), v3(at))?;
                json!({"group": r.group, "layers": r.layers, "faces": r.faces, "edges": r.edges, "units": r.units, "skipped": r.skipped, "labels": r.labels, "dimensions": r.dimensions, "focus": [r.focus[0].to_array(), r.focus[1].to_array()]})
            }
            Command::ImportDwg { path, units, at } => {
                let scratch = crate::safety::TempDir::new("sketchcraft-dwg")?;
                let tmp = scratch.path().to_path_buf();
                let dxf = crate::dwg::to_dxf(std::path::Path::new(&path), &tmp);
                let r = dxf.and_then(|d| {
                    let text = String::from_utf8_lossy(&std::fs::read(&d)?).into_owned();
                    let label = std::path::Path::new(&path).file_stem().map(|s| tidy_name(&s.to_string_lossy())).unwrap_or_else(|| "DWG drawing".into());
                    crate::dxf::import(m, &text, &label, units.as_deref(), v3(at))
                });
                let r = r?;
                json!({"group": r.group, "layers": r.layers, "faces": r.faces, "edges": r.edges, "units": r.units, "skipped": r.skipped, "labels": r.labels, "dimensions": r.dimensions, "focus": [r.focus[0].to_array(), r.focus[1].to_array()]})
            }
            Command::ExportDwg { path } => {
                let scratch = crate::safety::TempDir::new("sketchcraft-dwgout")?;
                let tmp = scratch.path().to_path_buf();
                let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "model".into());
                let dxf = tmp.join(format!("{stem}.dxf"));
                let r = crate::export2::write_dxf(&self.model, dxf.to_str().unwrap()).and_then(|n| crate::dwg::from_dxf(&dxf, std::path::Path::new(&path)).map(|_| n));
                json!({"path": path, "triangles": r?})
            }
            Command::Sheets { path, sheet, project, pages, blueprint, notes } => {
                let project = project.unwrap_or_else(|| std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Project".into()));
                json!({"path": path, "pages": crate::layout::write_sheets(&self.model, &path, &sheet, &project, pages, blueprint, &notes)?})
            }
            Command::ExportIfc { path, project } => {
                let project = project.unwrap_or_else(|| std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Project".into()));
                json!({"path": path, "elements": crate::ifc::write_ifc(&self.model, &path, &project)?})
            }
            Command::ExportDae { path } => json!({"path": path, "triangles": crate::export2::write_dae(&self.model, &path)?}),
            Command::ExportDxf { path } => json!({"path": path, "triangles": crate::export2::write_dxf(&self.model, &path)?}),
            Command::ExportPdf { path, title, opts } => {
                let img = render::render(&self.model, &opts);
                let t = title.unwrap_or_else(|| std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                crate::export2::write_pdf(&img, &format!("{t}  -  SketchCraft"), &path)?;
                json!({"path": path})
            }
            Command::ImportObj { path, text, up, scale, at, name } => {
                let text = match (text, &path) {
                    (Some(t), _) => t,
                    (None, Some(p)) => std::fs::read_to_string(p)?,
                    _ => bail!("import_obj needs a path or text"),
                };
                let p = path.as_deref().map(std::path::Path::new);
                let label = name.or_else(|| p.and_then(|p| p.file_stem()).map(|s| tidy_name(&s.to_string_lossy()))).unwrap_or_else(|| "Imported".into());
                let r = crate::import::import_obj(m, &text, p.and_then(|p| p.parent()), &label, up.eq_ignore_ascii_case("y"), scale, v3(at))?;
                json!({"instance": r.instance, "component": r.component, "groups": r.groups, "faces": r.faces, "skipped": r.skipped})
            }
            Command::ImportSkp { path, data, at, name } => {
                use base64::Engine as _;
                let bytes = match (data, &path) {
                    (Some(b64), _) => base64::engine::general_purpose::STANDARD.decode(b64.trim())?,
                    (None, Some(p)) => std::fs::read(p)?,
                    _ => bail!("import_skp needs a path or data"),
                };
                let label = name.unwrap_or_else(|| {
                    path.as_deref()
                        .and_then(|p| std::path::Path::new(p).file_stem())
                        .map(|s| crate::library::nice_name(&s.to_string_lossy()))
                        .unwrap_or_else(|| "SketchUp model".into())
                });
                let r = crate::skp::import(m, &bytes, v3(at), &label)?;
                json!({"group": r.group, "version": r.version, "faces": r.faces, "components": r.components, "placed": r.placed})
            }
            Command::ImportCloud { path, units, max_points } => {
                let scale = match units.as_deref().unwrap_or("m") {
                    "m" => 1.0,
                    "cm" => 0.01,
                    "mm" => 0.001,
                    "ft" => 0.3048,
                    "in" => 0.0254,
                    u => bail!("units must be m, cm, mm, ft or in (not {u})"),
                };
                let bytes = anyhow::Context::with_context(std::fs::read(&path), || format!("can't read {path}"))?;
                let (c, shift) = crate::cloud::read(&path, &bytes, scale, max_points.clamp(1, 50_000_000))?;
                let id = m.new_id();
                let (count, colored) = (c.len(), !c.colors.is_empty());
                m.clouds.insert(id, c);
                json!({"cloud": id, "points": count, "colored": colored, "shift": shift.to_array()})
            }
            Command::CloudData { id, max } => {
                let c = m.clouds.get(&id).ok_or_else(|| anyhow::anyhow!("no cloud {id}"))?;
                let (p, col) = c.thinned(max.clamp(1, 20_000_000));
                use base64::Engine as _;
                let b = base64::engine::general_purpose::STANDARD;
                json!({"points": b.encode(crate::cloud::f32_bytes(&p)), "colors": b.encode(&col), "count": p.len() / 3, "version": c.version})
            }
            Command::Cloud { id, name, hidden, delete } => {
                if delete {
                    m.clouds.remove(&id).ok_or_else(|| anyhow::anyhow!("no cloud {id}"))?;
                    json!({"deleted": id})
                } else {
                    let c = m.clouds.get_mut(&id).ok_or_else(|| anyhow::anyhow!("no cloud {id}"))?;
                    if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
                        c.name = n.trim().chars().take(80).collect();
                    }
                    if let Some(h) = hidden {
                        c.hidden = h;
                    }
                    json!({"cloud": id})
                }
            }
            Command::ImportIfc { path } => {
                let r = crate::ifcin::import(m, std::path::Path::new(&path))?;
                json!({"group": r.group, "elements": r.elements, "faces": r.faces, "storeys": r.storeys, "schema": r.schema})
            }
            Command::Library { dir } => {
                let d = dir.map(std::path::PathBuf::from).unwrap_or_else(crate::library::default_dir);
                std::fs::create_dir_all(&d).ok();
                json!({"dir": d.to_string_lossy(), "entries": crate::library::list(&d)})
            }
            Command::InsertFile { path, at, rotate } => {
                let def = crate::library::definition_for(m, &path)?;
                json!({"instance": m.place(def, v3(at), rotate, None)?, "component": def})
            }
            Command::SaveToLibrary { component, name, collection, dir } => {
                let model = crate::library::standalone(m, component)?;
                let d = dir.map(std::path::PathBuf::from).unwrap_or_else(crate::library::default_dir);
                let folder = match collection.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                    Some(c) => {
                        let c = crate::library::safe(c);
                        if crate::library::reserved(&c) {
                            bail!("{c:?} is a reserved folder name (Backups and Plugins are not library collections)");
                        }
                        d.join(c)
                    }
                    None => d.clone(),
                };
                std::fs::create_dir_all(&folder)?;
                if crate::library::is_protected(&folder, &d) {
                    bail!("that folder is the model's Backups or Plugins folder; the library never writes there");
                }
                let base = crate::library::safe(&name.unwrap_or_else(|| m.definitions[&component].name.clone()));
                let bytes = serde_json::to_string(&model)?;
                // a new file every time: never over an existing one (create_new also settles a race)
                let mut k = 1;
                let path = loop {
                    let path = folder.join(if k == 1 { format!("{base}.sketchcraft") } else { format!("{base} {k}.sketchcraft") });
                    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                        Ok(mut f) => {
                            use std::io::Write;
                            f.write_all(bytes.as_bytes())?;
                            break path;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && k < 10_000 => k += 1,
                        Err(e) => return Err(e.into()),
                    }
                };
                json!({"path": path.to_string_lossy()})
            }
            Command::LibraryAdd { paths, collection, dir } => {
                let d = dir.map(std::path::PathBuf::from).unwrap_or_else(crate::library::default_dir);
                json!({"added": crate::library::add_files(&d, &paths, collection.as_deref().unwrap_or(""), true)?})
            }
            Command::ImportFbx { path, data, at, name } => {
                use base64::Engine as _;
                let bytes = match (data, &path) {
                    (Some(b64), _) => base64::engine::general_purpose::STANDARD.decode(b64.trim())?,
                    (None, Some(p)) => std::fs::read(p)?,
                    _ => bail!("import_fbx needs a path or data"),
                };
                let label = name.or_else(|| path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()).map(|s| tidy_name(&s.to_string_lossy()))).unwrap_or_else(|| "FBX model".into());
                let r = crate::fbx::import(m, &bytes, &label, v3(at))?;
                json!({"instance": r.instance, "component": r.component, "meshes": r.meshes, "faces": r.faces})
            }
            Command::ExportObj { path } => {
                let tris = export::write_obj(&self.model, &path)?;
                json!({"path": path, "triangles": tris})
            }
            other => unreachable!("command routed to the wrong group: {other:?}"),
        })
    }
}
