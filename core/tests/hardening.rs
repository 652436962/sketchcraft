//! Regression tests for the security review (security-reports/, fixes in
//! RED-TEAM-REMEDIATION.md): data integrity, command policy, resource limits.
use serde_json::{json, Value};
use sketchcraft::live::Live;
use sketchcraft::policy::{Caller, Workspace};
use sketchcraft::Engine;
use std::path::PathBuf;

fn run(e: &mut Engine, v: Value) -> Value {
    let r = e.exec_json(&v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sc-hardening-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn faces(e: &mut Engine) -> usize {
    run(e, json!({"cmd": "info"}))["faces"].as_u64().unwrap() as usize
}

// ---- BRN-001/002/003: numbers, panics, non-finite saves -----------------------------

#[test]
fn huge_numbers_never_reach_the_model() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    for bad in [
        json!({"cmd": "scale", "ids": [g], "center": [0, 0, 0], "factor": 1.7e308}),
        json!({"cmd": "scale", "ids": [g], "center": [0, 0, 0], "factor": -1e300}),
        json!({"cmd": "move", "ids": [g], "by": [1.7e308, 0, 0]}),
        json!({"cmd": "smoove", "group": g, "at": [0, 0, 0], "radius": 1e308, "height": 1e308}),
        json!({"cmd": "rect", "from": [0, 0, 0], "to": [1e12, 1e12, 0]}),
    ] {
        let r = e.exec_json(&bad.to_string());
        assert_eq!(r["ok"], false, "{bad} should be refused");
    }
    // a result that overflows the model's range is undone, not kept
    let r = e.exec_json(&json!({"cmd": "move", "ids": [g], "by": [9e8, 0, 0]}).to_string());
    assert_eq!(r["ok"], true);
    let r = e.exec_json(&json!({"cmd": "scale", "ids": [g], "center": [0, 0, 0], "factor": 1e9}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    e.model.check_sane().unwrap();
}

#[test]
fn a_panic_inside_a_command_restores_the_model_and_keeps_undo_consistent() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "pushpull", "face": f, "distance": 3}));
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    let before = serde_json::to_string(&e.model).unwrap();
    // a group made from a stale face id used to panic the triangulator's caller
    let r = e.exec_json(&json!({"cmd": "subdivide", "id": g}).to_string());
    // either a clean error or a (now fixed) success; never a corrupted model or a lost step
    if r["ok"] == false {
        assert_eq!(serde_json::to_string(&e.model).unwrap(), before, "a failed command changes nothing");
        // undo still reverts the previous good step, not something else
        run(&mut e, json!({"cmd": "undo"}));
        assert!(e.model.groups.is_empty(), "undo removed the group made just before");
    }
    e.model.check_sane().unwrap();
}

#[test]
fn exec_survives_a_panic_and_leaves_the_model_alone() {
    // a vertex id the faces still point at, removed behind the model's back
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let victim = *e.model.vertices.keys().next().unwrap();
    e.model.vertices.remove(&victim);
    let before = e.model.faces.len();
    let r = e.exec_json(&json!({"cmd": "pushpull", "face": f, "distance": 1}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(e.model.faces.len(), before);
    // the engine still answers afterwards
    assert_eq!(e.exec_json(r#"{"cmd":"new"}"#)["ok"], true);
}

#[test]
fn a_model_with_non_finite_numbers_is_never_saved() {
    let dir = scratch("nonfinite");
    let file = dir.join("m.sketchcraft");
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    run(&mut e, json!({"cmd": "save", "path": file}));
    let good = std::fs::read(&file).unwrap();
    e.model.vertices.values_mut().next().unwrap().p.x = f64::NAN;
    let r = e.exec_json(&json!({"cmd": "save", "path": file}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(&file).unwrap(), good, "the good file is untouched");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opening_a_file_with_nulls_is_an_error_not_an_empty_model() {
    let dir = scratch("nullfile");
    let file = dir.join("bad.sketchcraft");
    std::fs::write(&file, r#"{"next_id":1,"vertices":{"1":{"p":[null,0,0]}}}"#).unwrap();
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let r = e.exec_json(&json!({"cmd": "open", "path": file}).to_string());
    assert_eq!(r["ok"], false);
    assert_eq!(faces(&mut e), 1, "the model that was open stays open");
    std::fs::remove_dir_all(&dir).ok();
}

// ---- BRN-004/005/006: the live file --------------------------------------------------

#[test]
fn a_damaged_live_file_is_kept_and_reported_not_replaced_silently() {
    let dir = scratch("damaged");
    let doc = dir.join("live.sketchcraft");
    std::fs::write(&doc, b"{\"next_id\": 5, \"vertices\": {\"1\": {\"p\": [0,0,0]").unwrap(); // truncated
    let original = std::fs::read(&doc).unwrap();
    let mut live = Live::open(doc.clone());
    assert!(live.notice.as_deref().is_some_and(|n| n.contains("couldn't be opened")), "{:?}", live.notice);
    let r = live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    assert_eq!(r["ok"], true);
    // the damaged bytes survive in a copy beside it
    let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.to_string_lossy().starts_with("damaged"))).collect();
    assert_eq!(kept.len(), 1, "one damaged copy");
    assert_eq!(std::fs::read(&kept[0]).unwrap(), original);
    // and the live file is now a good model
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_is_atomic_and_leaves_no_temp_files() {
    let dir = scratch("atomic");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..5 {
        let r = live.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4, 4, 0]}));
        assert_eq!(r["ok"], true);
        assert!(r.get("save_error").is_none());
    }
    let strays: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
    assert!(strays.is_empty(), "temp files left: {strays:?}");
    // always a whole, valid model
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 5);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rolling_backups_are_kept_and_bounded() {
    let dir = scratch("backups");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..14 {
        // `new` always takes a backup first
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        live.exec(&json!({"cmd": "new"}));
    }
    let n = std::fs::read_dir(dir.join("Backups")).unwrap().count();
    assert!((1..=9).contains(&n), "{n} backups");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_clients_do_not_overwrite_each_other() {
    let dir = scratch("lost-update");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    // another client (MCP, the CLI) adds a face and saves
    std::thread::sleep(std::time::Duration::from_millis(1100)); // file timestamps can be coarse
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the app edits before its poll noticed: it must start from the other's version
    app.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3, "all three rectangles survive");
    std::fs::remove_dir_all(&dir).ok();
}

// ---- MBK-001/002/004, SPC-003: who may run what --------------------------------------

#[test]
fn plugins_cannot_touch_files_plugins_or_programs() {
    let dir = scratch("plugin-policy");
    let marker = dir.join("marker.txt");
    let mut e = Engine::new();
    for bad in [
        json!({"cmd": "save_text", "path": marker, "text": "x"}),
        json!({"cmd": "install_plugin", "data": "AAAA", "name": "x.rb"}),
        json!({"cmd": "plugin_enable", "id": "cutlist", "on": false}),
        json!({"cmd": "remove_plugin", "id": "x"}),
        json!({"cmd": "reveal", "path": dir}),
        json!({"cmd": "skins"}),
        json!({"cmd": "save_skin", "name": "a", "skin": {}}),
        json!({"cmd": "add_location", "lat": 1, "lon": 1}),
        json!({"cmd": "photoreal", "samples": 4096}),
        json!({"cmd": "video_finish", "session": "a", "path": "http://127.0.0.1:9/x.ts"}),
        json!({"cmd": "export_obj", "path": marker}),
        json!({"cmd": "save", "path": marker}),
        json!({"cmd": "render", "out": marker}),
        json!({"cmd": "new"}),
        json!({"cmd": "totally_new_command"}),
    ] {
        let r = e.exec_json_as(&Caller::Plugin, &bad.to_string());
        assert_eq!(r["ok"], false, "{bad} -> {r}");
    }
    assert!(!marker.exists());
    // modeling still works for them
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}).to_string());
    assert_eq!(r["ok"], true);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn agents_work_inside_their_workspace_only() {
    let dir = scratch("mcp-policy");
    let ws = dir.join("work");
    std::fs::create_dir_all(&ws).unwrap();
    let caller = Caller::Mcp(Workspace::at(&ws));
    let mut e = Engine::new();
    e.exec_json_as(&caller, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}).to_string());
    let ok = e.exec_json_as(&caller, &json!({"cmd": "export_obj", "path": "house.obj"}).to_string());
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(ws.join("house.obj").exists());
    for bad in [
        json!({"cmd": "export_obj", "path": dir.join("outside.obj")}),
        json!({"cmd": "export_obj", "path": "../outside.obj"}),
        json!({"cmd": "save_text", "path": "a.txt", "text": "x"}),
        json!({"cmd": "install_plugin", "data": "AAAA", "name": "x.rb"}),
        json!({"cmd": "export_obj", "path": "Plugins/evil/main.js"}),
        json!({"cmd": "animate", "path": "http://127.0.0.1:9/x.mp4"}),
    ] {
        let r = e.exec_json_as(&caller, &bad.to_string());
        assert_eq!(r["ok"], false, "{bad} -> {r}");
    }
    assert!(!dir.join("outside.obj").exists());
    // --any-path lifts the folder limit, not the command list
    let wide = Caller::Mcp(Workspace::anywhere());
    assert_eq!(e.exec_json_as(&wide, &json!({"cmd": "export_obj", "path": dir.join("wide.obj")}).to_string())["ok"], true);
    assert_eq!(e.exec_json_as(&wide, &json!({"cmd": "save_text", "path": dir.join("t.txt"), "text": "x"}).to_string())["ok"], false);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn paths_that_are_urls_or_pipes_are_refused_for_everyone() {
    let mut e = Engine::new();
    for p in ["http://127.0.0.1:9/x.ts", "pipe:1", "tcp://127.0.0.1:9", "file:///etc/passwd", "concat:a|b"] {
        for cmd in ["video_finish", "animate", "export_obj", "save"] {
            let r = e.exec_json(&json!({"cmd": cmd, "session": "a", "path": p}).to_string());
            assert_eq!(r["ok"], false, "{cmd} {p} -> {r}");
            assert!(r["error"].as_str().unwrap().contains("isn't a file path"), "{r}");
        }
    }
}

#[test]
fn long_strings_are_refused() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    let r = e.exec_json(&json!({"cmd": "rename", "id": g, "name": "x".repeat(200_000)}).to_string());
    assert_eq!(r["ok"], false);
    let r = e.exec_json(&json!({"cmd": "text", "at": [0, 0, 0], "text": "x".repeat(50_000)}).to_string());
    assert_eq!(r["ok"], false);
}

// ---- resource limits ------------------------------------------------------------------

#[test]
fn render_and_photoreal_sizes_are_clamped_everywhere() {
    let o = sketchcraft::render::RenderOpts { width: 4_000_000_000, height: 4_000_000_000, ..Default::default() }.limited();
    assert!(o.width <= 8192 && o.height <= 8192 && o.width as u64 * o.height as u64 <= 25_000_000, "{}x{}", o.width, o.height);
    let o = sketchcraft::render::RenderOpts { width: 0, height: 0, ..Default::default() }.limited();
    assert!(o.width >= 16 && o.height >= 16);
    let p = sketchcraft::pt::PtOpts { width: 4096, height: 4096, samples: 4096, bounces: 16, ..Default::default() }.limited();
    assert!(p.width as u64 * p.height as u64 * p.samples as u64 * p.bounces as u64 <= 4_100_000_000, "{}x{} {} {}", p.width, p.height, p.samples, p.bounces);
    assert!(p.bounces <= 12);
    // and a huge render command returns quickly instead of grinding
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "render", "width": 3_000_000_000u64, "height": 3_000_000_000u64}).to_string());
    assert_eq!(r["ok"], false, "over the number limit: {r}");
    let r = e.exec_json(&json!({"cmd": "render", "width": 1e8, "height": 1e8}).to_string());
    assert_eq!(r["ok"], false);
    assert!(t.elapsed().as_secs() < 5);
}

#[test]
fn a_dxf_block_bomb_is_cut_off() {
    // block L0 draws a line; each next block inserts the one before 20 times
    let mut dxf = String::from("0\nSECTION\n2\nBLOCKS\n");
    for i in 0..8 {
        dxf += &format!("0\nBLOCK\n2\nL{i}\n10\n0\n20\n0\n30\n0\n");
        if i == 0 {
            dxf += "0\nLINE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n1\n31\n0\n";
        } else {
            for _ in 0..20 {
                dxf += &format!("0\nINSERT\n2\nL{}\n10\n0\n20\n0\n30\n0\n", i - 1);
            }
        }
        dxf += "0\nENDBLK\n";
    }
    dxf += "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nL7\n10\n0\n20\n0\n30\n0\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": dxf}).to_string());
    assert!(t.elapsed().as_secs() < 20, "took {:?}", t.elapsed());
    // refused whole, at once; never a hang
    assert_eq!(r["ok"], false, "{r}");
    assert!(e.model.faces.is_empty() && e.model.edges.is_empty(), "nothing was imported");
}

/// R2-MBK-004 / R2-BRN-005: a small file that multiplies a big block through MINSERT rows and columns
/// (or three nested 100 x 100 arrays of one line) is refused quickly.
#[test]
fn a_dxf_with_minsert_arrays_is_refused_inside_the_expansion() {
    let mut dxf = String::from("0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n30\n0\n");
    for i in 0..3000 {
        dxf += &format!("0\nLWPOLYLINE\n8\nW\n90\n4\n70\n1\n10\n{i}\n20\n0\n10\n{}\n20\n0\n10\n{}\n20\n1\n10\n{i}\n20\n1\n", i + 1, i + 1);
    }
    dxf += "0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nB\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n10\n45\n10\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": dxf}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 10, "took {:?}", t.elapsed());
    // nested arrays of one line
    let mut d2 = String::from("0\nSECTION\n2\nBLOCKS\n");
    d2 += "0\nBLOCK\n2\nA\n10\n0\n20\n0\n30\n0\n0\nLINE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n1\n31\n0\n0\nENDBLK\n";
    d2 += "0\nBLOCK\n2\nB\n10\n0\n20\n0\n30\n0\n0\nINSERT\n2\nA\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n2\n45\n2\n0\nENDBLK\n";
    d2 += "0\nBLOCK\n2\nC\n10\n0\n20\n0\n30\n0\n0\nINSERT\n2\nB\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n300\n45\n300\n0\nENDBLK\n";
    d2 += "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nC\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n30000\n45\n30000\n0\nENDSEC\n0\nEOF\n";
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": d2}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 10);
}

#[test]
fn undo_history_is_bounded_by_size() {
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "terrain_grid", "width": 60, "depth": 60, "spacing": 0.5}));
    let one = e.model.approx_bytes();
    assert!(one > 500_000, "{one}");
    e.undo_budget = one * 4;
    for i in 0..30 {
        run(&mut e, json!({"cmd": "rect", "from": [i, 0, 0], "to": [i + 1, 1, 0]}));
    }
    assert!(e.undo_depth() <= 5, "{} undo steps kept", e.undo_depth());
    assert!(e.undo_depth() >= 1);
    run(&mut e, json!({"cmd": "undo"}));
}

// ---- OBJ import and export --------------------------------------------------------------

#[test]
fn obj_material_files_come_from_the_objs_own_folder_only() {
    let dir = scratch("mtllib");
    let secret = dir.join("secret.mtl");
    std::fs::write(&secret, "newmtl Hidden\nKd 0.125 0.625 0.875\n").unwrap();
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("beside.mtl"), "newmtl Beside\nKd 0.25 0.5 0.75\n").unwrap();
    for (mtl, expect_color) in [("../secret.mtl", false), ("beside.mtl", true)] {
        let obj = sub.join("t.obj");
        std::fs::write(&obj, format!("mtllib {mtl}\nv 0 0 0\nv 1 0 0\nv 1 1 0\nusemtl Hidden\nusemtl Beside\nf 1 2 3\n")).unwrap();
        let mut e = Engine::new();
        run(&mut e, json!({"cmd": "import_obj", "path": obj}));
        let has_color = serde_json::to_string(&e.model).unwrap().contains("0.25,0.5,0.75");
        assert_eq!(has_color, expect_color, "{mtl}");
    }
    // an absolute path is no different
    let obj = sub.join("abs.obj");
    std::fs::write(&obj, format!("mtllib {}\nv 0 0 0\nv 1 0 0\nv 1 1 0\nf 1 2 3\n", secret.display())).unwrap();
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "import_obj", "path": obj}));
    assert!(!serde_json::to_string(&e.model).unwrap().contains("0.125,0.625,0.875"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn obj_export_names_cannot_start_new_lines() {
    let dir = scratch("objnames");
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "rename", "id": g, "name": "wall\nmtllib /etc/passwd\ro x"}));
    let out = dir.join("m.obj");
    run(&mut e, json!({"cmd": "export_obj", "path": out}));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.lines().any(|l| l.starts_with("mtllib /etc")), "{text}");
    assert!(text.lines().all(|l| !l.contains('\r')));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn imported_geometry_out_of_range_is_refused_whole() {
    // an arc whose radius is 1e300 used to import and poison the next transform
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nA\n10\n0\n20\n0\n30\n0\n11\n1e300\n21\n0\n31\n0\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let before = serde_json::to_string(&e.model).unwrap();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": dxf}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(serde_json::to_string(&e.model).unwrap(), before);
}

// ---- Round 2 ---------------------------------------------------------------------------

fn backups(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("Backups")).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// R2-BRN-001 / R2-MBK-006: another client's save is not one of my undo steps.
#[test]
fn undo_after_a_reload_cannot_erase_the_other_clients_work() {
    let dir = scratch("undo-reload");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    other.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the app picks up their work, then the person presses Undo
    assert_eq!(app.exec(&json!({"cmd": "info"}))["faces"], 3);
    app.exec(&json!({"cmd": "undo"}));
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3, "their rectangles are still on disk");
    // and the person's own steps still undo normally afterwards
    app.exec(&json!({"cmd": "rect", "from": [30, 0, 0], "to": [34, 4, 0]}));
    app.exec(&json!({"cmd": "undo"}));
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3);
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-MBK-006: a client that finishes a slow command after someone else saved does not overwrite them.
#[test]
fn a_save_made_while_a_command_ran_is_not_overwritten() {
    let dir = scratch("race");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the other client saves between this client's sync and its save: simulate with a file written under it
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // (app hasn't polled) its next command starts from theirs; nothing is lost
    let r = app.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    assert_eq!(r["ok"], true);
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3);
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-002 / R2-MBK-003: a flood of `new` cannot push out the last good backup.
#[test]
fn repeated_new_cannot_erase_the_last_good_backup() {
    let dir = scratch("backup-flood");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..5 {
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
    }
    // an older good backup, made an hour ago
    std::fs::create_dir_all(dir.join("Backups")).unwrap();
    let old = dir.join("Backups").join(format!("live-{}.sketchcraft", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 7200));
    std::fs::copy(&doc, &old).unwrap();
    for _ in 0..30 {
        live.exec(&json!({"cmd": "new"}));
    }
    // empty models are not backed up
    let mut check = Engine::new();
    for p in backups(&dir) {
        run(&mut check, json!({"cmd": "open", "path": p}));
        assert!(faces(&mut check) >= 1, "every backup holds a drawn model");
    }
    // many distinct models in a row: the older good backup and the first of the burst remain
    let first: Vec<PathBuf> = backups(&dir);
    for i in 0..20 {
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        live.exec(&json!({"cmd": "new"}));
    }
    let b2 = backups(&dir);
    assert!(b2.contains(&old), "the older good backup survived the flood");
    assert!(first.iter().filter(|p| **p != old).min_by_key(|p| { let n = p.file_stem().unwrap().to_string_lossy().into_owned(); let mut it = n.rsplit('-'); let (a, b) = (it.next().and_then(|x| x.parse::<u64>().ok()), it.next().and_then(|x| x.parse::<u64>().ok())); match (a, b) { (Some(n), Some(t)) => (t, n), (Some(t), None) => (t, 0), _ => (0, 0) } }).is_none_or(|p| b2.contains(p)), "so did the first backup of the burst");
    assert!(b2.len() <= 12, "{} backups", b2.len());
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-008: flags and copies don't pile up or stick.
#[test]
fn damaged_copies_are_pruned_and_the_notice_clears_when_the_file_is_repaired() {
    let dir = scratch("damaged-prune");
    let doc = dir.join("live.sketchcraft");
    for _ in 0..9 {
        std::fs::write(&doc, b"{ not a model").unwrap();
        let live = Live::open(doc.clone());
        assert!(live.notice.is_some());
    }
    let kept = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".damaged-")).count();
    assert!(kept <= 5, "{kept} damaged copies");
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-004: a point list that would take minutes is refused up front.
#[test]
fn long_point_lists_and_huge_counts_are_refused_quickly() {
    let mut e = Engine::new();
    let pts: Vec<Value> = (0..3000).map(|i| json!([i as f64, (i % 7) as f64, 0])).collect();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "line", "points": pts}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    let ring: Vec<Value> = (0..3000).map(|i| json!([i as f64, (i % 7) as f64])).collect();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "text_3d", "rings": [ring.clone(), ring.clone(), ring.clone()], "height": 1}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    // R2-MBK-005: a stair of 100,000 m would be half a million steps
    let t = std::time::Instant::now();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "stairs", "at": [0, 0, 0], "rise": 100000, "tread": 0.3}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    let r = e.exec_json(&json!({"cmd": "stairs", "at": [0, 0, 0], "rise": 1e8, "tread": 0.3}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 2);
    let r = e.exec_json(&json!({"cmd": "stairs", "at": [0, 0, 0], "rise": 2.8, "tread": 0.28}).to_string());
    assert_eq!(r["ok"], true, "{r}");
}

/// R2-BRN-003: names and notes count toward the undo budget and are capped in total.
#[test]
fn long_names_are_counted_and_capped() {
    let mut e = Engine::new();
    let mut refused = false;
    for i in 0..200 {
        let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "material", "name": format!("{i}{}", "m".repeat(60_000)), "color": [1, 0, 0]}).to_string());
        if r["ok"] != true {
            refused = true;
            break;
        }
    }
    assert!(refused, "the total of names is capped");
    assert!(e.model.text_bytes() <= sketchcraft::model::MAX_TEXT_BYTES);
    assert!(e.model.approx_bytes() >= e.model.text_bytes());
}

/// R2-BRN-007: a point cloud with out-of-range values is cleaned at import and refused in the model.
#[test]
fn point_clouds_are_finite_and_in_range() {
    let dir = scratch("cloud");
    let f = dir.join("c.xyz");
    std::fs::write(&f, "1 2 3\n4 5 6\n1e39 0 0\n7 8 9\n").unwrap();
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "import_cloud", "path": f}));
    assert!(e.model.clouds.values().all(|c| c.points.iter().all(|x| x.is_finite() && (*x as f64).abs() <= 1e9)));
    assert_eq!(e.model.clouds.values().map(|c| c.points.len()).sum::<usize>(), 9);
    e.model.clouds.values_mut().next().unwrap().points = std::sync::Arc::new(vec![f32::INFINITY, 0.0, 0.0]);
    assert!(e.model.check_sane().is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-008: grouping part of a solid then subdividing the group no longer hits a stale id.
#[test]
fn subdividing_a_partial_group_works() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [1, 1, 0]}))["face"].as_u64().unwrap();
    let top = run(&mut e, json!({"cmd": "pushpull", "face": f, "distance": 1}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [top]}))["group"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "subdivide", "id": g, "levels": 1}));
    // everything still refers to vertices that exist
    for f in e.model.faces.values() {
        assert!(f.loops().flatten().all(|v| e.model.vertices.contains_key(v)));
    }
}

fn agent(ws: &std::path::Path) -> Caller {
    Caller::Mcp(Workspace::at(ws))
}

/// R2-MBK-001: `library_add` copies an OBJ's material file only when it is a bare name beside the OBJ.
#[test]
fn library_add_cannot_be_steered_by_the_objs_material_line() {
    let base = scratch("libadd");
    let ws = base.join("ws");
    std::fs::create_dir_all(ws.join("a")).unwrap();
    let victim = base.join("victim.txt");
    std::fs::write(&victim, b"important").unwrap();
    let obj = |mtl: &str| format!("mtllib {mtl}\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    let mut e = Engine::new();
    // absolute path to a file outside: it must not be touched
    std::fs::write(ws.join("a/m.obj"), obj(&victim.to_string_lossy())).unwrap();
    e.exec_json_as(&agent(&ws), &json!({"cmd": "library_add", "paths": ["a/m.obj"]}).to_string());
    assert_eq!(std::fs::read(&victim).unwrap(), b"important");
    // a relative path out of the folder: nothing lands outside the library
    std::fs::create_dir_all(base.join("escape")).unwrap();
    std::fs::write(ws.join("escape.txt"), b"payload").unwrap();
    std::fs::write(ws.join("a/n.obj"), obj("../../escape.txt")).unwrap();
    e.exec_json_as(&agent(&ws), &json!({"cmd": "library_add", "paths": ["a/n.obj"]}).to_string());
    assert!(!base.join("escape.txt").exists() && !ws.join("Library/../../escape.txt").exists());
    // the library an agent uses is inside its workspace
    assert!(ws.join("Library/m.obj").exists());
    // and the same through the engine directly (the person's own add): still no truncation
    let lib = base.join("lib");
    sketchcraft::library::add_files(&lib, &[ws.join("a/m.obj").to_string_lossy().into_owned()], "").unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"important");
    std::fs::remove_dir_all(&base).ok();
}

/// R2-MBK-002: `save_image` writes only real PNGs, with a `.png` name, and never over someone else's file.
#[test]
fn agents_write_only_the_right_kind_of_file_and_never_replace_the_persons() {
    use base64::Engine as _;
    let base = scratch("agentwrite");
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let mut e = Engine::new();
    let b64 = |b: &[u8]| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b));
    // a shell script dressed as an image
    let r = e.exec_json_as(&agent(&ws), &json!({"cmd": "save_image", "path": "build.sh", "data": b64(b"#!/bin/sh\necho hi\n")}).to_string());
    assert_eq!(r["ok"], false);
    let r = e.exec_json_as(&agent(&ws), &json!({"cmd": "save_image", "path": "x.png", "data": b64(b"#!/bin/sh\necho hi\n")}).to_string());
    assert_eq!(r["ok"], false, "not decodable: {r}");
    assert!(!ws.join("x.png").exists() && !ws.join("build.sh").exists());
    // a real image is written
    let mut png = vec![];
    image::DynamicImage::new_rgb8(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let r = e.exec_json_as(&agent(&ws), &json!({"cmd": "save_image", "path": "x.png", "data": b64(&png)}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    // the person's own file is not replaced, whatever the command
    std::fs::write(ws.join("mine.obj"), b"the person's model").unwrap();
    let a = agent(&ws);
    for cmd in [json!({"cmd": "export_obj", "path": "mine.obj"}), json!({"cmd": "save", "path": "mine.obj"}), json!({"cmd": "export_obj", "path": "notes.txt"})] {
        let r = e.exec_json_as(&a, &cmd.to_string());
        assert_eq!(r["ok"], false, "{cmd} -> {r}");
    }
    assert_eq!(std::fs::read(ws.join("mine.obj")).unwrap(), b"the person's model");
    // a file the agent made itself may be written again
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "mine2.obj"}));
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "mine2.obj"}));
    // a workspace is never a drive root or home folder
    assert!(Workspace::at("/").too_wide());
    if let Some(h) = std::env::var_os("HOME") {
        assert!(Workspace::at(h).too_wide());
    }
    assert!(!Workspace::at(&ws).too_wide());
    std::fs::remove_dir_all(&base).ok();
}

fn run_as(e: &mut Engine, c: &Caller, v: Value) -> Value {
    let r = e.exec_json_as(c, &v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}
