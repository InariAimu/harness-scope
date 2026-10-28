use anyhow::{Context, Result};
use harness_scope_core::Engine;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

const TRAY_ID: &str = "harness-scope";
const SETTINGS_KEY: &str = "desktop_preferences";
static OPEN_REQUESTED: AtomicBool = AtomicBool::new(false);

fn startup_command_path(path: &std::path::Path) -> Result<String> {
    let path = path
        .to_str()
        .context("Executable path is not valid Unicode")?;
    anyhow::ensure!(!path.contains('"'), "Invalid executable path");
    Ok(format!("\"{path}\""))
}

fn autolaunch() -> Result<auto_launch::AutoLaunch> {
    // auto-launch's Windows backend writes the supplied command verbatim.
    // Quote the executable and explicitly use HKCU, even when running elevated.
    Ok(auto_launch::AutoLaunchBuilder::new()
        .set_app_name("Harness Scope")
        .set_app_path(&startup_command_path(&std::env::current_exe()?)?)
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
        .set_args(&["--autostart"])
        .build()?)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub silent_startup: bool,
    pub minimize_to_tray: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            silent_startup: false,
            minimize_to_tray: true,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Patch {
    launch_on_startup: Option<bool>,
    silent_startup: Option<bool>,
    minimize_to_tray: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(flatten)]
    preferences: Preferences,
    launch_on_startup: bool,
    tray_available: bool,
    warning: Option<String>,
}

pub struct DesktopState {
    preferences: Mutex<Preferences>,
    minimize_to_tray: AtomicBool,
    tray_available: AtomicBool,
    quitting: AtomicBool,
    warning: Option<String>,
}

fn should_show_on_start(preferences: &Preferences, tray_available: bool, requested: bool) -> bool {
    requested || !tray_available || !preferences.silent_startup
}

fn should_hide_on_close(preferences: &Preferences, tray_available: bool, quitting: bool) -> bool {
    preferences.minimize_to_tray && tray_available && !quitting
}

fn save_preferences(engine: &Engine, previous: &Preferences, patch: &Patch) -> Result<Preferences> {
    let mut next = previous.clone();
    if let Some(value) = patch.silent_startup {
        next.silent_startup = value;
    }
    if let Some(value) = patch.minimize_to_tray {
        next.minimize_to_tray = value;
    }
    engine.lock()?.set_meta(SETTINGS_KEY, &next)?;
    Ok(next)
}

pub fn request_open(app: &tauri::AppHandle) {
    // A second launch can arrive while the first is still opening its database.
    OPEN_REQUESTED.store(true, Ordering::Release);
    if app.try_state::<DesktopState>().is_some() {
        if let Err(error) = show_main(app) {
            report_error(app, &error.to_string());
        }
    }
}

pub fn show_main(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.set_skip_taskbar(false)?;
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
        let _ = window.emit("desktop-restored", ());
    }
    Ok(())
}

fn report_error(app: &tauri::AppHandle, message: &str) {
    eprintln!("Desktop: {message}");
    let _ = app.emit("desktop-error", message);
}

fn menu(app: &tauri::AppHandle, language: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let zh = language.starts_with("zh");
    let open = MenuItem::with_id(
        app,
        "open",
        if zh {
            "打开 Harness Scope"
        } else {
            "Open Harness Scope"
        },
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        if zh { "退出" } else { "Quit" },
        true,
        None::<&str>,
    )?;
    Menu::with_items(app, &[&open, &quit])
}

pub fn set_menu_language(app: &tauri::AppHandle, language: &str) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu(app, language)?))?;
    }
    Ok(())
}

pub fn remove_tray(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
}

fn quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DesktopState>() {
        state.quitting.store(true, Ordering::Release);
    }
    remove_tray(app);
    app.exit(0);
}

pub fn setup(app: &tauri::AppHandle, engine: &Engine) -> Result<()> {
    let (preferences, language, mut warning) = {
        let store = engine.lock()?;
        let language = store.config()?.language;
        match store.meta::<Preferences>(SETTINGS_KEY) {
            Ok(value) => (value.unwrap_or_default(), language, None),
            Err(error) => (
                Preferences::default(),
                language,
                Some(format!("Could not load desktop preferences: {error}")),
            ),
        }
    };
    // Refresh an existing registration after the executable was moved. Never
    // enable autostart just because the application was launched.
    let autolaunch = autolaunch()?;
    match autolaunch.is_enabled() {
        Ok(true) => {
            if let Err(error) = autolaunch.enable() {
                warning = Some(format!("Could not update startup registration: {error}"));
            }
        }
        Ok(false) => {}
        Err(error) => warning = Some(format!("Could not read startup registration: {error}")),
    }
    let tray = (|| -> Result<()> {
        TrayIconBuilder::with_id(TRAY_ID)
            .icon(
                app.default_window_icon()
                    .context("Missing application icon")?
                    .clone(),
            )
            .tooltip("Harness Scope")
            .menu(&menu(app, &language)?)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "open" => request_open(app),
                "quit" => quit(app),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    request_open(tray.app_handle());
                }
            })
            .build(app)?;
        Ok(())
    })();
    let tray_available = tray.is_ok();
    if let Err(error) = tray {
        warning = Some(format!("Could not create system tray: {error}"));
    }
    let show = should_show_on_start(
        &preferences,
        tray_available,
        OPEN_REQUESTED.load(Ordering::Acquire),
    );
    app.manage(DesktopState {
        minimize_to_tray: AtomicBool::new(preferences.minimize_to_tray),
        preferences: Mutex::new(preferences),
        tray_available: AtomicBool::new(tray_available),
        quitting: AtomicBool::new(false),
        warning,
    });
    if show || OPEN_REQUESTED.load(Ordering::Acquire) {
        show_main(app)?;
    }
    Ok(())
}

pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        if let Some(state) = app.try_state::<DesktopState>() {
            let hide = should_hide_on_close(
                &Preferences {
                    silent_startup: false,
                    minimize_to_tray: state.minimize_to_tray.load(Ordering::Acquire),
                },
                state.tray_available.load(Ordering::Acquire),
                state.quitting.load(Ordering::Acquire),
            );
            if hide {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    report_error(app, &error.to_string());
                }
            } else {
                remove_tray(app);
            }
        }
    }
}

fn read_settings(app: &tauri::AppHandle) -> Result<Settings> {
    let state = app.state::<DesktopState>();
    let preferences = state
        .preferences
        .lock()
        .map_err(|_| anyhow::anyhow!("Desktop settings lock poisoned"))?
        .clone();
    Ok(Settings {
        preferences,
        launch_on_startup: autolaunch()?.is_enabled()?,
        tray_available: state.tray_available.load(Ordering::Acquire),
        warning: state.warning.clone(),
    })
}

#[tauri::command]
pub async fn desktop_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || read_settings(&app).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_desktop_settings(
    app: tauri::AppHandle,
    engine: tauri::State<'_, Arc<Engine>>,
    patch: Patch,
) -> Result<Settings, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Settings> {
        // UI changes one switch per request. Keeping OS changes separate avoids
        // pretending the registry and SQLite share a transaction.
        if let Some(enabled) = patch.launch_on_startup {
            anyhow::ensure!(
                patch.silent_startup.is_none() && patch.minimize_to_tray.is_none(),
                "Change autostart separately"
            );
            if enabled {
                autolaunch()?.enable()?;
            } else {
                autolaunch()?.disable()?;
            }
        } else {
            anyhow::ensure!(
                patch.silent_startup.is_some() || patch.minimize_to_tray.is_some(),
                "Empty desktop settings patch"
            );
            let state = app.state::<DesktopState>();
            let mut preferences = state
                .preferences
                .lock()
                .map_err(|_| anyhow::anyhow!("Desktop settings lock poisoned"))?;
            *preferences = save_preferences(&engine, &preferences, &patch)?;
            state
                .minimize_to_tray
                .store(preferences.minimize_to_tray, Ordering::Release);
        }
        read_settings(&app)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_quotes_paths_with_spaces_and_unicode() {
        let path = std::path::Path::new(r"C:\应用 Programs\Harness Scope\scope.exe");
        assert_eq!(
            startup_command_path(path).unwrap(),
            r#""C:\应用 Programs\Harness Scope\scope.exe""#
        );
    }

    #[test]
    fn defaults_and_all_start_close_combinations() {
        assert_eq!(
            serde_json::from_str::<Preferences>("{}").unwrap(),
            Preferences::default()
        );
        assert!(!Preferences::default().silent_startup);
        assert!(Preferences::default().minimize_to_tray);
        for silent in [false, true] {
            for minimize in [false, true] {
                for tray in [false, true] {
                    let p = Preferences {
                        silent_startup: silent,
                        minimize_to_tray: minimize,
                    };
                    assert_eq!(should_show_on_start(&p, tray, false), !silent || !tray);
                    assert!(should_show_on_start(&p, tray, true));
                    assert_eq!(should_hide_on_close(&p, tray, false), minimize && tray);
                    assert!(!should_hide_on_close(&p, tray, true));
                }
            }
        }
    }

    #[test]
    fn preferences_survive_restart_without_changing_core_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.db");
        let engine = Engine::open(&path).unwrap();
        let config = engine.execute("snapshot", serde_json::json!({})).unwrap()["config"].clone();
        let patch = serde_json::from_value(serde_json::json!({"silentStartup":true})).unwrap();
        let saved = save_preferences(&engine, &Preferences::default(), &patch).unwrap();
        assert!(saved.minimize_to_tray);
        assert_eq!(
            engine.execute("snapshot", serde_json::json!({})).unwrap()["config"],
            config
        );
        drop(engine);
        let engine = Engine::open(&path).unwrap();
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .meta::<Preferences>(SETTINGS_KEY)
                .unwrap(),
            Some(saved)
        );
    }

    #[test]
    fn reject_unknown_or_invalid_preferences() {
        assert!(serde_json::from_str::<Patch>(r#"{"silentStartup":"true"}"#).is_err());
        assert!(serde_json::from_str::<Patch>(r#"{"serverToken":"secret"}"#).is_err());
    }
}
