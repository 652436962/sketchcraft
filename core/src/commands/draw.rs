//! Drawing and shaping geometry: the drawing tools, push/pull, move/rotate/scale, solids, terrain and the building helpers.
//! (Arms of `Engine::run`; each runs on the model being edited: the open component's definition inside `edit`.)

use super::*;

impl Engine {
    pub(super) fn run_draw(&mut self, cmd: Command) -> Result<Value> {
        let m: &mut Model = active_model!(self);
        Ok(match cmd {
            Command::New { figure } => {
                *m = Model::default();
                match figure.as_deref() {
                    None | Some("" | "none") => json!({}),
                    Some(key @ ("person" | "beaver")) => {
                        let def = crate::assets::definition(m, key)?;
                        let id = m.place(def, DVec3::ZERO, 0.0, None)?;
                        json!({"figure": id})
                    }
                    Some(other) => bail!("figure is person, beaver or none (got {other:?})"),
                }
            }
            Command::Rect { from, to } => {
                let (a, b) = (v3(from), v3(to));
                let d = b - a;
                let k = (0..3).find(|&k| d[k].abs() < 1e-9);
                let Some(k) = k else { bail!("rect corners must share one coordinate (use polygon for tilted shapes)") };
                let (i, j) = ((k + 1) % 3, (k + 2) % 3);
                if d[i].abs() < 1e-9 || d[j].abs() < 1e-9 {
                    bail!("rect has zero width");
                }
                let mut p1 = a;
                p1[i] = b[i];
                let mut p3 = a;
                p3[j] = b[j];
                let f = m.add_polygon(&[a, p1, b, p3])?;
                json!({"face": f})
            }
            Command::Polygon { points } => {
                let pts: Vec<DVec3> = points.into_iter().map(v3).collect();
                json!({"face": m.add_polygon(&pts)?})
            }
            Command::Line { points } => {
                if points.len() < 2 {
                    bail!("a line needs 2 points");
                }
                let (mut edges, mut faces) = (vec![], vec![]);
                for w in points.windows(2) {
                    let r = m.draw_line(v3(w[0]), v3(w[1]))?;
                    edges.extend(r.edges);
                    faces.extend(r.faces);
                }
                json!({"edges": edges, "faces": faces, "face": faces.last()})
            }
            Command::Circle { center, radius, normal, segments } => {
                let f = m.circle(v3(center), v3(normal), radius, segments)?;
                let ring = m.pts(&m.face(f)?.outer.clone());
                m.curve_through(&ring, true);
                json!({"face": f})
            }
            Command::Arc { start, end, bulge, normal, segments } => {
                let pts = Model::arc_points(v3(start), v3(end), bulge, v3(normal), segments)?;
                let mut edges = vec![];
                let mut faces = vec![];
                for w in pts.windows(2) {
                    let r = m.draw_line(w[0], w[1])?;
                    edges.extend(r.edges);
                    faces.extend(r.faces);
                }
                m.curve_through(&pts, false);
                json!({"edges": edges, "faces": faces, "face": faces.last()})
            }
            Command::Offset { face, distance } => json!({"face": m.offset(face, distance)?}),
            Command::FollowMe { face, points, edges, around } => {
                let (path, closed) = if let Some(p) = points {
                    let pts: Vec<DVec3> = p.into_iter().map(v3).collect();
                    let closed = pts.len() > 2 && pts[0].distance(*pts.last().unwrap()) < 1e-9;
                    (pts, closed)
                } else if let Some(es) = edges {
                    m.edge_path(&es)?
                } else if let Some(f) = around {
                    (m.pts(&m.face(f)?.outer.clone()), true)
                } else {
                    bail!("follow_me needs points, edges or around");
                };
                let faces = m.follow_me(face, &path, closed)?;
                json!({"faces": faces})
            }
            Command::Pushpull { face, distance, toward } => {
                let distance = match toward {
                    Some(t) if m.normal(face).dot(v3(t)) < 0.0 => -distance,
                    _ => distance,
                };
                let ids = m.pushpull(face, distance)?;
                json!({"face": ids[0], "created": &ids[1..]})
            }
            Command::Move { ids, by } => json!({"moved": m.translate(&ids, v3(by))?}),
            Command::Rotate { ids, center, axis, angle } => json!({"moved": m.rotate(&ids, v3(center), v3(axis), angle)?}),
            Command::Scale { ids, center, factor } => {
                let f = match factor {
                    Factor::Uniform(k) => DVec3::splat(k),
                    Factor::Axes(a) => v3(a),
                };
                json!({"moved": m.scale(&ids, v3(center), f)?})
            }
            Command::Duplicate { ids, by } => json!({"ids": m.duplicate(&ids, v3(by))?}),
            Command::Array { ids, by, count } => json!({"ids": m.array(&ids, v3(by), count)?}),
            Command::Delete { ids } => {
                m.delete(&ids)?;
                json!({})
            }
            Command::Reverse { ids } => {
                let faces = m.faces_of(&ids)?;
                for f in &faces {
                    m.faces.get_mut(f).unwrap().reverse();
                }
                json!({"faces": faces})
            }
            Command::RaiseWalls { group, height, max_thickness, sill, head } => {
                let r = crate::dxf::raise_walls_with(m, group, height, max_thickness, sill, head)?;
                json!({"walls": r.walls, "windows": r.windows, "layers": r.layers})
            }
            Command::Weld { edges } => json!({"curve": m.weld(&edges)?}),
            Command::ExplodeCurve { edges } => json!({"edges": m.explode_curve(&edges)}),
            Command::OrientFaces { face } => json!({"flipped": m.orient_faces(face)?}),
            Command::Text3d { rings, at, right, up, depth, name } => {
                // its own group, like SketchUp's 3D Text
                let g = m.begin_group(name.unwrap_or_else(|| "3D Text".into()));
                let r = m.text_3d(&rings, v3(at), v3(right), v3(up), depth);
                m.active_group = m.groups.get(&g).and_then(|g| g.parent);
                let faces = r?;
                json!({"group": g, "faces": faces})
            }
            Command::Subd { name, points, faces, materials, creases, levels, editable, symmetry } => {
                let n = faces.len();
                let mut ed = crate::subd::Editable { points, faces, materials: (0..n).map(|i| materials.get(i).cloned().flatten()).collect(), creases, levels: levels.clamp(0, 5), symmetry };
                if ed.symmetry.is_none() {
                    ed.detect_symmetry();
                }
                let g = crate::subd::build(m, ed.cage(), ed.levels, &name.unwrap_or_else(|| "Smooth surface".into()))?;
                let sym = ed.symmetry;
                if editable {
                    m.cages.insert(g, ed);
                }
                json!({"group": g, "faces": m.faces.values().filter(|f| f.group == Some(g)).count(), "symmetry": sym})
            }
            Command::CageEdit { id, moves, mirror, crease, uncrease, op, levels, symmetry } => {
                m.check_unlocked(&[id])?;
                let Some(mut ed) = m.cages.get(&id).cloned() else { bail!("{id} isn't an editable subdivision object") };
                match symmetry.as_ref() {
                    Some(Value::String(s)) if s == "off" => ed.symmetry = None,
                    Some(Value::String(s)) if s == "auto" => ed.detect_symmetry(),
                    Some(Value::Number(a)) => {
                        let axis = a.as_u64().unwrap_or(1).min(2) as usize;
                        let mid = ed.points.iter().map(|p| p[axis]).fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
                        ed.symmetry = Some(crate::subd::Symmetry { axis, center: (mid.0 + mid.1) / 2.0 });
                    }
                    _ => {}
                }
                if !moves.is_empty() {
                    let mv: Vec<(usize, [f64; 3])> = moves.iter().map(|m| (m[0] as usize, [m[1], m[2], m[3]])).collect();
                    ed.move_points(&mv, mirror)?;
                }
                // creases, mirrored when symmetric
                let partners = ed.mirrors();
                let mirror_edges = |es: &[[usize; 2]]| -> Vec<[usize; 2]> {
                    let mut all = es.to_vec();
                    if mirror {
                        for e in es {
                            if let (Some(Some(a)), Some(Some(b))) = (partners.get(e[0]), partners.get(e[1])) {
                                all.push([*a, *b]);
                            }
                        }
                    }
                    all
                };
                ed.set_creases(&mirror_edges(&crease), true);
                ed.set_creases(&mirror_edges(&uncrease), false);
                let mut out = json!({});
                if let Some(op) = op {
                    match op {
                        CageOp::InsertLoop { edge, t } => out["points"] = json!(ed.insert_loop(edge[0], edge[1], t)?),
                        CageOp::Extrude { faces, distance } => out["faces"] = json!(ed.extrude(&faces, distance)?),
                        CageOp::DeleteFaces { faces } => ed.delete_faces(&faces),
                        CageOp::Merge { points } => ed.merge(&points)?,
                    }
                }
                if let Some(l) = levels {
                    ed.levels = l.clamp(0, 5);
                }
                crate::subd::fill(m, ed.cage(), ed.levels, id)?;
                out["cage_points"] = json!(ed.points.len());
                out["cage_faces"] = json!(ed.faces.len());
                out["symmetry"] = json!(ed.symmetry);
                m.cages.insert(id, ed);
                out
            }
            Command::Subdivide { id, levels, keep_sharp } => {
                let Some(old) = m.groups.get(&id).cloned() else { bail!("{id} is not a group") };
                m.check_unlocked(&[id])?;
                let cage = crate::subd::cage_of(m, id, keep_sharp)?;
                m.delete(&[id])?;
                let prev = m.active_group;
                m.active_group = old.parent;
                let g = crate::subd::build(m, cage, levels, &old.name)?;
                m.active_group = prev;
                if let Some(x) = m.groups.get_mut(&g) {
                    x.parent = old.parent;
                    x.tag = old.tag;
                }
                json!({"group": g, "faces": m.faces.values().filter(|f| f.group == Some(g)).count()})
            }
            Command::DoorSwing { hinge, strike, toward } => {
                let (h, s, t) = (v3(hinge), v3(strike), v3(toward));
                let across = DVec3::new(s.x - h.x, s.y - h.y, 0.0);
                let w = across.length();
                if w < 0.1 {
                    bail!("the hinge and strike points are the same");
                }
                // open: square to the opening, on the side `toward` is
                let mut perp = DVec3::Z.cross(across).normalize();
                if (t - h).dot(perp) < 0.0 {
                    perp = -perp;
                }
                let id = m.new_id();
                m.annotations.push(crate::model::Annotation::Swing { id, hinge: h, from: s, to: h + perp * w });
                json!({"swing": id})
            }
            Command::TerrainFromContours { ids, spacing } => json!({"group": m.terrain_from_contours(&ids, spacing)?}),
            Command::TerrainGrid { at, width, depth, spacing } => json!({"group": m.terrain_grid(v3(at), width, depth, spacing)?}),
            Command::Smoove { group, at, radius, height } => json!({"moved": m.smoove(group, v3(at), radius, height)?}),
            Command::Stamp { group, face, offset } => json!({"moved": m.stamp(group, face, offset)?}),
            Command::Drape { group, ids } => json!({"lines": m.drape(group, &ids)?}),
            Command::FlipEdge { edge } => {
                m.flip_edge(edge)?;
                json!({})
            }
            Command::Bevel { edges, size, segments } => {
                let groups = m.bevel(&edges, size, segments)?;
                json!({"groups": groups, "group": groups.first()})
            }
            Command::Divide { edge, count } => json!({"points": m.divide(edge, count)?}),
            Command::Slice { id, origin, normal } => {
                let groups = m.slice(id, v3(origin), v3(normal))?;
                json!({"groups": groups, "group": groups.first()})
            }
            Command::Intersect { ids, with } => json!({"lines": m.intersect(&ids, with != "selection")?}),
            Command::Flip { ids, axis } => json!({"moved": m.flip(&ids, axis_index(&axis)?)?}),
            Command::Solid { op, ids } => {
                let groups = crate::csg::solid(m, &op, &ids)?;
                json!({"groups": groups, "group": groups.first()})
            }
            // on a scratch copy: checking adds paint entries it doesn't need to keep
            Command::Volume { id } => match crate::csg::solid_info(&mut m.clone(), id) {
                Some(v) => json!({"solid": true, "volume": v}),
                None => json!({"solid": false}),
            },
            Command::Wall { points, thickness, height, closed } => {
                let pts: Vec<DVec3> = points.into_iter().map(v3).collect();
                json!({"group": m.wall(&pts, thickness, height, closed)?})
            }
            Command::Stairs { at, rise, direction, width, tread } => {
                let (g, risers) = m.stairs(v3(at), direction, width, rise, tread)?;
                json!({"group": g, "risers": risers, "riser_height": rise / risers as f64})
            }
            Command::Roof { face, kind, pitch, overhang, thickness } => json!({"group": m.roof(face, &kind, pitch, overhang, thickness)?}),
            Command::Column { at, size, height, round } => json!({"group": m.column(v3(at), size, height, round)?}),
            Command::Insert { asset, at, rotate, host } => {
                let def = crate::assets::definition(m, &asset)?;
                let instance = m.place(def, v3(at), rotate, None)?;
                let opening = place_cut(m, instance, host);
                json!({"instance": instance, "component": def, "opening": opening})
            }
            Command::Assets => json!({"assets": crate::assets::ASSETS.iter().map(|a| json!({"key": a.key, "name": a.name, "about": a.about})).collect::<Vec<_>>()}),
            other => unreachable!("command routed to the wrong group: {other:?}"),
        })
    }
}
