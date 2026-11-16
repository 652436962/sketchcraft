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
    assert!((1..=8).contains(&n), "{n} backups");
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
    assert!(p.width as u64 * p.height as u64 * p.samples as u64 * p.bounces as u64 <= 2_600_000_000, "{}x{} {} {}", p.width, p.height, p.samples, p.bounces);
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
    // either imported what fit under the budget, or refused; never a hang
    assert!(r["ok"] == true || r["ok"] == false);
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
