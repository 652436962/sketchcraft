//! Installing, turning off and removing plugins (in a scratch Plugins folder).
use serde_json::{json, Value};
use sketchcraft::Engine;
use std::io::Write;

fn run(e: &mut Engine, v: Value) -> Value {
    e.exec_json(&v.to_string())
}

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn install_turn_off_and_remove_a_plugin() {
    let home = std::env::temp_dir().join(format!("sc-plugins-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("SKETCHCRAFT_DOC", home.join("live.sketchcraft"));
    let mut e = Engine::new();

    let manifest = r#"{"name":"Stair Maker","version":"1.2.0","commands":[{"id":"stairs","label":"Stairs…","icon":"stairs.svg"}]}"#;
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
    // zipped inside a folder, with a junk file that isn't unpacked
    let z = zip_of(&[("stair-maker/plugin.json", manifest), ("stair-maker/main.js", "sketchcraft.command('stairs', () => {});"), ("stair-maker/stairs.svg", svg), ("stair-maker/run.exe", "MZ")]);
    let r = run(&mut e, json!({"cmd":"install_plugin","data": b64(&z)}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["id"], "stair-maker");
    let folder = home.join("Plugins/stair-maker");
    assert!(folder.join("main.js").is_file() && !folder.join("run.exe").exists());

    let list = run(&mut e, json!({"cmd":"plugins"}));
    let p = list["plugins"].as_array().unwrap().iter().find(|p| p["id"] == "stair-maker").unwrap().clone();
    assert_eq!(p["enabled"], true);
    assert_eq!(p["version"], "1.2.0");
    assert!(p["commands"][0]["icon"].as_str().unwrap().starts_with("data:image/svg+xml;base64,"));

    // off, and on again
    run(&mut e, json!({"cmd":"plugin_enable","id":"stair-maker","on":false}));
    let list = run(&mut e, json!({"cmd":"plugins"}));
    assert_eq!(list["plugins"].as_array().unwrap().iter().find(|p| p["id"] == "stair-maker").unwrap()["enabled"], false);
    run(&mut e, json!({"cmd":"plugin_enable","id":"stair-maker","on":true}));

    // installing again replaces it (the old one is kept aside), and turns it on
    run(&mut e, json!({"cmd":"plugin_enable","id":"stair-maker","on":false}));
    let r = run(&mut e, json!({"cmd":"install_plugin","data": b64(&z)}));
    assert_eq!(r["replaced"], true);
    let list = run(&mut e, json!({"cmd":"plugins"}));
    assert_eq!(list["plugins"].as_array().unwrap().iter().find(|p| p["id"] == "stair-maker").unwrap()["enabled"], true);

    // a zip that reaches outside its folder is refused, and nothing is written
    let evil = zip_of(&[("plugin.json", r#"{"name":"Evil","commands":[]}"#), ("main.js", ""), ("../../escape.js", "x")]);
    let r = run(&mut e, json!({"cmd":"install_plugin","data": b64(&evil)}));
    assert_eq!(r["ok"], false, "{r}");
    assert!(!home.join("escape.js").exists() && !home.join("Plugins/evil").exists());
    // and one without plugin.json
    let r = run(&mut e, json!({"cmd":"install_plugin","data": b64(&zip_of(&[("main.js", "")]))}));
    assert!(r["error"].as_str().unwrap().contains("plugin.json"));
    // a built-in can't be removed
    assert_eq!(run(&mut e, json!({"cmd":"remove_plugin","id":"cutlist"}))["ok"], false);

    // removing moves it aside
    let r = run(&mut e, json!({"cmd":"remove_plugin","id":"stair-maker"}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(!folder.exists() && home.join("Plugins/.removed").is_dir());
    let list = run(&mut e, json!({"cmd":"plugins"}));
    assert!(!list["plugins"].as_array().unwrap().iter().any(|p| p["id"] == "stair-maker"));
    std::fs::remove_dir_all(&home).ok();
}
