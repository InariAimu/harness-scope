#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use harness_scope_core::Engine;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;

#[tauri::command]
async fn command(
    engine: tauri::State<'_, Arc<Engine>>,
    name: String,
    args: Option<Value>,
) -> Result<Value, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        engine
            .execute(&name, args.unwrap_or(serde_json::json!({})))
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                window.set_decorations(false)?;
                // Config dimensions are logical; request physical client pixels at local DPI.
                window.set_size(tauri::PhysicalSize::new(1500, 1000))?;
                window.center()?;
            }
            let root = app.path().app_data_dir()?;
            std::fs::create_dir_all(&root)?;
            let engine = Arc::new(Engine::open(&root.join("harness-scope.db"))?);
            engine.start_background();
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![command])
        .run(tauri::generate_context!())
        .expect("Harness Scope failed to start");
}
