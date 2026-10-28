#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use harness_scope_core::Engine;
#[cfg(target_os = "windows")]
use harness_scope_desktop::desktop;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;

#[tauri::command]
async fn command(
    app: tauri::AppHandle,
    engine: tauri::State<'_, Arc<Engine>>,
    name: String,
    args: Option<Value>,
) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    let language = if name == "save_config" || name == "patch_config" {
        args.as_ref()
            .and_then(|value| {
                value[if name == "patch_config" {
                    "patch"
                } else {
                    "config"
                }]["language"]
                    .as_str()
            })
            .map(str::to_owned)
    } else {
        None
    };
    #[cfg(not(target_os = "windows"))]
    let _ = app;
    let engine = engine.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        engine
            .execute(&name, args.unwrap_or(serde_json::json!({})))
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())??;
    #[cfg(target_os = "windows")]
    if let Some(language) = language {
        // The preference is already saved; a menu error must not roll it back.
        if let Err(error) = desktop::set_menu_language(&app, &language) {
            eprintln!("Tray language: {error}");
        }
    }
    Ok(result)
}
fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "windows")]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|arg| arg == "--autostart") {
                desktop::request_open(app);
            }
        }))
        .on_window_event(desktop::on_window_event);
    let builder = builder.plugin(tauri_plugin_dialog::init()).setup(|app| {
        #[cfg(target_os = "windows")]
        if let Some(window) = app.get_webview_window("main") {
            window.set_decorations(false)?;
            // Preserve the layout viewport across DPI scales (1500x1000 at 150%).
            window.set_size(tauri::LogicalSize::new(1000.0, 667.0))?;
            window.center()?;
        }
        let root = app.path().app_data_dir()?;
        std::fs::create_dir_all(&root)?;
        let engine = Arc::new(Engine::open(&root.join("harness-scope.db"))?);
        #[cfg(target_os = "windows")]
        desktop::setup(app.handle(), &engine)?;
        #[cfg(not(target_os = "windows"))]
        if let Some(window) = app.get_webview_window("main") {
            window.show()?;
        }
        engine.start_background();
        app.manage(engine);
        Ok(())
    });
    #[cfg(target_os = "windows")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        command,
        desktop::desktop_settings,
        desktop::save_desktop_settings
    ]);
    #[cfg(not(target_os = "windows"))]
    let builder = builder.invoke_handler(tauri::generate_handler![command]);
    builder
        .build(tauri::generate_context!())
        .expect("Harness Scope failed to start")
        .run(|app, event| {
            #[cfg(target_os = "windows")]
            if matches!(event, tauri::RunEvent::Exit) {
                desktop::remove_tray(app);
            }
            #[cfg(not(target_os = "windows"))]
            let _ = (app, event);
        });
}
