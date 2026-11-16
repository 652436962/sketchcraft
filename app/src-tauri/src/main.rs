//! SketchCraft desktop backend. Holds the engine and gives the UI three calls:
//!
//!   exec(cmd)  run one engine command (the UI never edits geometry itself)
//!   scene()    triangles, edges and layers for the 3D view
//!   sync()     pick up changes another client (Claude over MCP) saved
//!
//! The model lives in ~/Documents/SketchCraft/live.sketchcraft (or $SKETCHCRAFT_DOC).
//! The app saves there after every change and reloads when someone else
//! writes it, so you and an AI agent can work on the same model at once.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sketchcraft::live::{default_doc, Live};
use serde_json::Value;
use std::sync::Mutex;

struct App(Mutex<Live>);

/// The engine, even if an earlier call panicked while holding it (a bug in one
/// command shouldn't lock you out of the rest of the app).
fn engine<'a>(state: &'a tauri::State<'_, App>) -> std::sync::MutexGuard<'a, Live> {
    state.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run `f`, turning a panic into an error reply instead of a crash.
fn guarded<T>(f: impl FnOnce() -> T, fallback: impl FnOnce(String) -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown".into());
            eprintln!("sketchcraft: a command panicked: {msg}");
            fallback(msg)
        }
    }
}

#[tauri::command]
fn exec(state: tauri::State<App>, cmd: Value) -> Value {
    let mut live = engine(&state);
    guarded(|| live.exec(&cmd), |msg| serde_json::json!({"ok": false, "error": format!("SketchCraft hit a bug running that ({msg}). Undo if anything looks wrong.")}))
}

#[tauri::command]
fn scene(state: tauri::State<App>) -> Value {
    let live = engine(&state);
    guarded(|| live.scene(), |_| serde_json::json!({"ok": false}))
}

#[tauri::command]
fn sync(state: tauri::State<App>) -> bool {
    let mut live = engine(&state);
    guarded(|| live.sync(), |_| false)
}

/// Running from the portable ZIP (a PORTABLE.txt beside the program)? Then
/// updates are a new ZIP to download, not an installer.
#[tauri::command]
fn portable() -> bool {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("PORTABLE.txt").is_file())).unwrap_or(false)
}

/// Open the guide in the browser. The address is fixed, never from the page.
#[tauri::command]
fn open_guide() -> bool {
    open_url("https://github.com/jasonsutter87/sketchcraft/blob/main/docs/guide/README.md")
}

fn open_url(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.is_ok()
}

/// Open the download page in the browser (for installs the updater can't
/// replace, like a .deb or .rpm). The address is fixed, never from the page.
#[tauri::command]
fn open_releases() -> bool {
    const URL: &str = "https://github.com/jasonsutter87/sketchcraft/releases/latest";
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(URL).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(URL).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(URL).spawn();
    r.is_ok()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        // signed self-updates from GitHub Releases (see app/src/updater.tsx)
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(App(Mutex::new(Live::open(default_doc()))))
        .invoke_handler(tauri::generate_handler![exec, scene, sync, open_releases, portable, open_guide])
        .run(tauri::generate_context!())
        .expect("error while running SketchCraft");
}
