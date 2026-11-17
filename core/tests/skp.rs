//! The SketchUp importer against a tiny hand-built legacy .skp stream: one
//! component definition holding one triangle, the way SketchUp 7 writes it.

mod skp_fixture;
use skp_fixture::*;
use sketchcraft::Engine;
use serde_json::{json, Value};

fn triangle_skp() -> Vec<u8> {
    let mut w = W(Vec::new());
    w.0.extend([0xFF, 0xFE, 0xFF, 0x0E]);
    for u in "SketchUp Model".encode_utf16() {
        w.u16(u);
    }
    w.ustr("{7.1.6087}");
    w.0.extend([0u8; 64]);
    // definition #9's entities: [def + 1][0][count]
    let base = 11u32;
    w.u32(base - 1).u16(0).u32(1);
    // face (class base, object base+1)
    w.class("CFace").header().f64(0.0).f64(0.0).f64(1.0).f64(0.0).u32(1);
    // loop (class base+2, object base+3)
    w.class("CLoop").u32(0x0101_0000);
    // edge use 1 (class base+4, object base+5) with a new edge (class base+6, object base+7)
    w.class("CEdgeUse").u16(0);
    w.class("CEdge").header();
    w.class("CVertex").vertex(0.0, 0.0, 0.0); // class base+8, object base+9
    w.u16(0x8000 | (base + 8) as u16).vertex(100.0, 0.0, 0.0); // object base+10
    w.u16(0); // no curve
    w.u8(0).u16((base + 3) as u16);
    // edge use 2 (base+11), new edge (base+12): ref base+10 -> new vertex (base+13)
    w.u16(0x8000 | (base + 4) as u16).u16(0);
    w.u16(0x8000 | (base + 6) as u16).header();
    w.u16((base + 10) as u16);
    w.u16(0x8000 | (base + 8) as u16).vertex(0.0, 100.0, 0.0);
    w.u16(0);
    w.u8(0).u16((base + 3) as u16);
    // edge use 3 (base+14), new edge (base+15) closing the loop
    w.u16(0x8000 | (base + 4) as u16).u16(0);
    w.u16(0x8000 | (base + 6) as u16).header();
    w.u16((base + 13) as u16).u16((base + 9) as u16).u16(0);
    w.u8(0).u16((base + 3) as u16);
    w.u16(0); // end of loop
    w.u16(0); // back material
    w.0.extend([0u8; 32]);
    w.0
}

fn run(e: &mut Engine, v: Value) -> Value {
    let r = e.exec_json(&v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}

#[test]
fn imports_a_hand_built_legacy_skp() {
    let dir = std::env::temp_dir().join(format!("sketchcraft-skp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tri.skp");
    std::fs::write(&path, triangle_skp()).unwrap();
    let mut e = Engine::new();
    let r = run(&mut e, json!({"cmd":"import_skp","path":path.to_string_lossy()}));
    assert_eq!(r["faces"], 1);
    assert_eq!(r["version"], "{7.1.6087}");
    // nothing places the definition, so it's placed once at the origin
    assert_eq!(r["placed"], 1);
    let faces = e.model.world_faces();
    assert_eq!(faces.len(), 1);
    let area = sketchcraft::geom::area(&faces[0].outer);
    let inch = 0.0254;
    assert!((area - 0.5 * (100.0 * inch) * (100.0 * inch)).abs() < 1e-9, "100-inch right triangle, in meters: {area}");
    assert!(faces[0].normal.z > 0.99, "faces up like the stored plane");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn rejects_non_sketchup_files() {
    let mut e = Engine::new();
    let r = e.exec_json(&json!({"cmd":"import_skp","data":"aGVsbG8gd29ybGQ="}).to_string());
    assert_eq!(r["ok"], false);
}

// ---- modern (SketchUp 2013+) files: header + "VFF" + ZIP with model.dat ----

#[test]
fn imports_a_modern_skp() {
    use base64::Engine as _;
    let mut e = Engine::new();
    let data = base64::engine::general_purpose::STANDARD.encode(modern_skp());
    let r = run(&mut e, json!({"cmd":"import_skp","data":data}));
    assert_eq!(r["version"], "{26.2.0}");
    assert_eq!(r["faces"], 1);
    assert_eq!(r["placed"], 1);
    let faces = e.model.world_faces();
    assert_eq!(faces.len(), 1, "the face inside the nested component draws");
    let inch = 0.0254;
    let area = sketchcraft::geom::area(&faces[0].outer);
    assert!((area - 0.5 * (100.0 * inch) * (100.0 * inch)).abs() < 1e-9, "{area}");
    let min_x = faces[0].outer.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    assert!((min_x - 10.0 * inch).abs() < 1e-9, "placed by the instance transform: {min_x}");
    assert_eq!(faces[0].color, Some([1.0, 0.0, 0.0]), "color from material.xml");
}

/// SketchUp 27 names a placement's definition by id, not GUID (issue #1:
/// files loaded their components but placed none, so nothing showed).
#[test]
fn imports_a_sketchup_27_skp() {
    use base64::Engine as _;
    let mut e = Engine::new();
    let data = base64::engine::general_purpose::STANDARD.encode(skp_fixture::su27_skp());
    let r = run(&mut e, json!({"cmd":"import_skp","data":data}));
    assert_eq!(r["version"], "{27.0.0}");
    assert_eq!(r["placed"], 1, "{r}");
    let faces = e.model.world_faces();
    assert_eq!(faces.len(), 1, "the face inside the nested component draws");
    let min_x = faces[0].outer.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    assert!((min_x - 10.0 * 0.0254).abs() < 1e-9, "placed by the instance transform: {min_x}");
}
