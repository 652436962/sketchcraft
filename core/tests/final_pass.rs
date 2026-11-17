//! Final pre-0.3.6 pass: built-in assets and library parts are found by identity, not by a name
//! anyone can give a component (SPECTER-001), and a model file never carries a folder path (CASHOUT-004).

use serde_json::{json, Value};
use sketchcraft::Engine;

fn ok(e: &mut Engine, v: Value) -> Value {
    let r = e.exec_json(&v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}

#[test]
fn a_component_named_door_does_not_take_over_the_built_in_door() {
    let mut e = Engine::new();
    let face = ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}))["face"].clone();
    let planted = ok(&mut e, json!({"cmd": "make_component", "ids": [face], "name": "Door"}))["component"].clone();
    let placed = ok(&mut e, json!({"cmd": "insert", "asset": "door", "at": [10, 0, 0]}));
    assert_ne!(placed["component"], planted, "{placed}");
    // the built-in is reused the second time
    let again = ok(&mut e, json!({"cmd": "insert", "asset": "door", "at": [20, 0, 0]}));
    assert_eq!(again["component"], placed["component"]);
    // and a component that claims the built-in's reserved key but has other geometry isn't reused either
    let key = e.model.definitions[&placed["component"].as_u64().unwrap().try_into().unwrap()].source.clone();
    assert_eq!(key.as_deref(), Some("builtin:door"));
    let pid: u32 = planted.as_u64().unwrap().try_into().unwrap();
    e.model.definitions.get_mut(&pid).unwrap().source = key;
    e.model.definitions.get_mut(&pid).unwrap().cuts = true;
    let third = ok(&mut e, json!({"cmd": "insert", "asset": "door", "at": [30, 0, 0]}));
    assert_eq!(third["component"], placed["component"]);
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sketchcraft-final-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn part(path: &std::path::Path, size: f64) {
    let mut e = Engine::new();
    let face = ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [size, size, 0]}))["face"].clone();
    ok(&mut e, json!({"cmd": "make_component", "ids": [face], "name": "Chair"}));
    let m = sketchcraft::library::standalone(&e.model, *e.model.definitions.keys().next().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string(&m).unwrap()).unwrap();
}

#[test]
fn library_parts_are_reused_by_content_and_the_saved_model_holds_no_folder_path() {
    let dir = scratch("lib");
    let (a, b) = (dir.join("one").join("chair.sketchcraft"), dir.join("two").join("chair.sketchcraft"));
    std::fs::create_dir_all(a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    part(&a, 1.0);
    part(&b, 2.0);
    let mut e = Engine::new();
    let p1 = ok(&mut e, json!({"cmd": "insert_file", "path": a.to_string_lossy(), "at": [0, 0, 0]}));
    let p2 = ok(&mut e, json!({"cmd": "insert_file", "path": a.to_string_lossy(), "at": [5, 0, 0]}));
    let p3 = ok(&mut e, json!({"cmd": "insert_file", "path": b.to_string_lossy(), "at": [9, 0, 0]}));
    assert_eq!(p1["component"], p2["component"], "the same file is reused");
    assert_ne!(p1["component"], p3["component"], "same name, other contents: its own component");
    let saved = serde_json::to_string(&e.model).unwrap();
    assert!(!saved.contains(dir.to_string_lossy().as_ref()), "a folder path is in the model file");
    assert!(!saved.contains("sketchcraft-final-lib"), "a folder name is in the model file");
    // the scene an agent sees names no folder either
    let scene = ok(&mut e, json!({"cmd": "components"})).to_string();
    assert!(!scene.contains("sketchcraft-final-lib"), "{scene}");
    // a changed file is a new component
    part(&a, 3.0);
    let p4 = ok(&mut e, json!({"cmd": "insert_file", "path": a.to_string_lossy(), "at": [0, 5, 0]}));
    assert_ne!(p4["component"], p1["component"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_older_model_file_with_an_absolute_source_loads_and_loses_the_folder() {
    let mut e = Engine::new();
    let face = ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}))["face"].clone();
    ok(&mut e, json!({"cmd": "make_component", "ids": [face], "name": "Chair"}));
    let mut v: Value = serde_json::to_value(&e.model).unwrap();
    for d in v["definitions"].as_object_mut().unwrap().values_mut() {
        d["source"] = json!("/Users/someone/Documents/SketchCraft/Library/chair.sketchcraft");
    }
    let old = serde_json::to_string(&v).unwrap();
    let m: sketchcraft::model::Model = serde_json::from_str(&old).unwrap();
    let src = m.definitions.values().next().unwrap().source.clone().unwrap();
    assert_eq!(src, "chair.sketchcraft");
    let win: sketchcraft::model::Model = serde_json::from_str(&old.replace("/Users/someone/Documents/SketchCraft/Library/", "\\\\\\\\?\\\\C:\\\\Users\\\\x\\\\")).unwrap();
    assert_eq!(win.definitions.values().next().unwrap().source.as_deref(), Some("x\\\\chair.sketchcraft").map(|_| "chair.sketchcraft"));
}
