mod config;
mod error;
mod recipes;
mod webviews;

use config::{
    load_preferences, load_services, load_state, save_state, AppState, Preferences, Service,
    ServicesLoadInfo,
};
use error::TauriumError;
use recipes::Recipe;
use std::collections::{HashMap, HashSet};
use tauri::menu::{ContextMenu, MenuBuilder, MenuItemBuilder};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};
use webviews::WebviewState;

const TAURI_INVOKE_SHIM: &str = r#"
if (!window.__TAURI__ && window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {
  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
    }
  };
}
"#;

#[derive(serde::Serialize)]
pub struct ApplyServicesResponse {
    pub filtered_url_count: usize,
}

#[tauri::command]
fn get_recipes() -> Vec<Recipe> {
    recipes::load_recipes()
}

#[tauri::command]
fn get_services(state: tauri::State<WebviewState>) -> Result<Vec<Service>, TauriumError> {
    let services = state
        .services
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
    Ok(services.clone())
}

#[tauri::command]
fn switch_service(
    app: tauri::AppHandle,
    state: tauri::State<WebviewState>,
    id: String,
) -> Result<(), TauriumError> {
    webviews::switch_to(&app, &state, &id)?;

    let app_state = AppState {
        last_active_service: Some(id),
    };
    save_state(&state.app_data_dir, &app_state)?;

    Ok(())
}

#[tauri::command]
fn get_last_active_service(state: tauri::State<WebviewState>) -> Option<String> {
    let app_state = load_state(&state.app_data_dir);
    app_state.last_active_service
}

#[tauri::command]
fn save_services_cmd(
    state: tauri::State<WebviewState>,
    services: Vec<Service>,
) -> Result<(), TauriumError> {
    config::save_services(&state.app_data_dir, &services)?;
    {
        let mut stored = state
            .services
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
        *stored = services.clone();
    }
    eprintln!("[Taurium] Services saved ({} services)", services.len());
    Ok(())
}

#[tauri::command]
fn open_settings(
    app: tauri::AppHandle,
    state: tauri::State<WebviewState>,
) -> Result<(), TauriumError> {
    webviews::show_settings(&app, &state)
}

#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    eprintln!("[Taurium] Restarting app...");
    tauri::process::restart(&app.env());
}

#[tauri::command]
fn reload_service(
    app: tauri::AppHandle,
    state: tauri::State<WebviewState>,
    id: String,
) -> Result<(), TauriumError> {
    webviews::reload_service_webview(&app, &state, &id)
}

#[tauri::command]
fn get_badge_counts(
    state: tauri::State<WebviewState>,
) -> Result<HashMap<String, u32>, TauriumError> {
    let badge_counts = state
        .badge_counts
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
    Ok(badge_counts.clone())
}

#[tauri::command]
fn get_service_url(
    state: tauri::State<WebviewState>,
    id: String,
) -> Result<Option<String>, TauriumError> {
    let services = state
        .services
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
    Ok(services.iter().find(|s| s.id == id).map(|s| s.url.clone()))
}

#[tauri::command]
fn apply_services(
    app: tauri::AppHandle,
    state: tauri::State<WebviewState>,
) -> Result<ApplyServicesResponse, TauriumError> {
    let loaded = load_services(&state.app_data_dir)?;
    webviews::apply_service_changes(&app, &state, loaded.services)?;
    Ok(ApplyServicesResponse {
        filtered_url_count: loaded.filtered_url_count,
    })
}

#[tauri::command]
fn get_services_load_info(state: tauri::State<WebviewState>) -> ServicesLoadInfo {
    state.services_load_info.clone()
}

#[tauri::command]
fn show_service_context_menu(app: tauri::AppHandle, id: String) -> Result<(), TauriumError> {
    eprintln!("[Taurium] Showing context menu for: {}", id);

    // Store the target service id for the menu event handler
    *app.state::<ContextMenuTarget>()
        .0
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))? = Some(id);

    let reload_item = MenuItemBuilder::with_id("ctx_reload", "Reload").build(&app)?;
    let zoom_in_item = MenuItemBuilder::with_id("ctx_zoom_in", "Zoom In").build(&app)?;
    let zoom_out_item = MenuItemBuilder::with_id("ctx_zoom_out", "Zoom Out").build(&app)?;
    let open_item = MenuItemBuilder::with_id("ctx_open_browser", "Open in browser").build(&app)?;

    let menu = MenuBuilder::new(&app)
        .item(&reload_item)
        .item(&zoom_in_item)
        .item(&zoom_out_item)
        .item(&open_item)
        .build()?;

    let window = app.get_window("main").ok_or(TauriumError::WindowNotFound)?;
    menu.popup(window)?;

    Ok(())
}

#[tauri::command]
fn get_preferences(state: tauri::State<WebviewState>) -> Preferences {
    load_preferences(&state.app_data_dir)
}

#[tauri::command]
fn save_preferences_cmd(
    app: tauri::AppHandle,
    state: tauri::State<WebviewState>,
    prefs: Preferences,
) -> Result<String, TauriumError> {
    config::save_preferences(&state.app_data_dir, &prefs)?;
    let prefs_json = serde_json::to_string(&prefs)?;

    let sidebar = app
        .get_webview("sidebar")
        .ok_or_else(|| TauriumError::WebviewNotFound("sidebar".to_string()))?;

    let js = format!(
        "window.__applyPreferences && window.__applyPreferences({})",
        prefs_json
    );
    sidebar.eval(&js)?;

    eprintln!("[Taurium] Preferences saved and applied to sidebar");
    Ok(prefs_json)
}

// Holds the service ID targeted by the context menu
pub struct ContextMenuTarget(std::sync::Mutex<Option<String>>);

fn persist_and_apply_service_zoom(
    app: &tauri::AppHandle,
    state: &WebviewState,
    service_id: &str,
    delta: f64,
) {
    let new_zoom = {
        let mut services = match state.services.lock() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[Taurium] Mutex poisoned: {}", e);
                return;
            }
        };
        let Some(svc) = services.iter_mut().find(|s| s.id == service_id) else {
            return;
        };
        let cur = svc.zoom.unwrap_or(1.0);
        let new_z = ((cur + delta) * 10.0).round() / 10.0;
        let new_z = new_z.clamp(0.5, 2.0);
        svc.zoom = if (new_z - 1.0).abs() < 0.001 {
            None
        } else {
            Some(new_z)
        };
        let z = svc.zoom;
        config::save_services(&state.app_data_dir, &services).unwrap_or_else(|err| {
            eprintln!("[Taurium] Failed to save service zoom: {err}");
        });
        z
    };
    if let Some(wv) = app.get_webview(service_id) {
        webviews::apply_service_body_zoom(&wv, new_zoom);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");

            let (services, services_load_info) = match load_services(&app_data_dir) {
                Ok(loaded) => {
                    if loaded.created_defaults {
                        eprintln!("[Taurium] Created default services.json");
                    }
                    (
                        loaded.services,
                        ServicesLoadInfo {
                            filtered_url_count: loaded.filtered_url_count,
                            load_error: None,
                        },
                    )
                }
                Err(err) => {
                    eprintln!("[Taurium] Failed to load services: {err}");
                    (
                        Vec::new(),
                        ServicesLoadInfo {
                            filtered_url_count: 0,
                            load_error: Some(err.to_string()),
                        },
                    )
                }
            };

            // Register state FIRST
            let webview_state = WebviewState {
                created_ids: std::sync::Mutex::new(Vec::new()),
                active_id: std::sync::Mutex::new(None),
                app_data_dir,
                services: std::sync::Mutex::new(services.clone()),
                navigated: std::sync::Mutex::new(HashSet::new()),
                last_activity: std::sync::Mutex::new(HashMap::new()),
                badge_counts: std::sync::Mutex::new(HashMap::new()),
                services_load_info,
            };
            app.manage(webview_state);
            app.manage(ContextMenuTarget(std::sync::Mutex::new(None)));

            // Create main window
            let window = tauri::window::WindowBuilder::new(app, "main")
                .title("Taurium")
                .inner_size(1200.0, 800.0)
                .min_inner_size(400.0, 300.0)
                .build()?;

            let inner = window.inner_size()?;
            let scale = window.scale_factor()?;
            let w = inner.width as f64 / scale;
            let h = inner.height as f64 / scale;

            // Add sidebar webview
            let sidebar_builder = tauri::webview::WebviewBuilder::new(
                "sidebar",
                WebviewUrl::App("index.html".into()),
            )
            .initialization_script(TAURI_INVOKE_SHIM);
            let _sidebar_webview = window.add_child(
                sidebar_builder,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(webviews::SIDEBAR_WIDTH, h),
            )?;

            let content_width = w - webviews::SIDEBAR_WIDTH;

            // Pre-create settings webview (hidden)
            let settings_builder = tauri::webview::WebviewBuilder::new(
                "settings",
                WebviewUrl::App("settings.html".into()),
            )
            .initialization_script(TAURI_INVOKE_SHIM);
            let settings_webview = window.add_child(
                settings_builder,
                LogicalPosition::new(webviews::SIDEBAR_WIDTH, 0.0),
                LogicalSize::new(content_width, h),
            )?;
            settings_webview.hide()?;
            eprintln!("[Taurium] Settings webview created (hidden)");

            // Pre-create ALL service webviews with about:blank (lazy loading).
            // Must be done here in setup() because add_child() deadlocks
            // when called from command handlers on Windows (WebView2 STA issue).
            for service in &services {
                eprintln!(
                    "[Taurium] Pre-creating webview: {} (about:blank, lazy)",
                    service.id
                );
                webviews::create_service_webview(app.handle(), service)?;
            }

            // Listen for window resize events
            let app_handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Resized(_) = event {
                    let state = app_handle.state::<WebviewState>();
                    webviews::resize_all_webviews(&app_handle, &state);
                }
            });

            // Handle context menu events
            app.on_menu_event(move |app_handle_evt, event| {
                let menu_id = event.id().0.as_str();
                let target_id = match app_handle_evt.state::<ContextMenuTarget>().0.lock() {
                    Ok(guard) => guard.clone(),
                    Err(e) => {
                        eprintln!("[Taurium] Mutex poisoned: {}", e);
                        return;
                    }
                };

                if let Some(service_id) = target_id {
                    match menu_id {
                        "ctx_reload" => {
                            eprintln!("[Taurium] Context menu: reload {}", service_id);
                            let state = app_handle_evt.state::<WebviewState>();
                            webviews::reload_service_webview(app_handle_evt, &state, &service_id)
                                .ok();
                        }
                        "ctx_zoom_in" => {
                            eprintln!("[Taurium] Context menu: zoom in {}", service_id);
                            let state = app_handle_evt.state::<WebviewState>();
                            persist_and_apply_service_zoom(
                                app_handle_evt,
                                &state,
                                &service_id,
                                0.1,
                            );
                        }
                        "ctx_zoom_out" => {
                            eprintln!("[Taurium] Context menu: zoom out {}", service_id);
                            let state = app_handle_evt.state::<WebviewState>();
                            persist_and_apply_service_zoom(
                                app_handle_evt,
                                &state,
                                &service_id,
                                -0.1,
                            );
                        }
                        "ctx_open_browser" => {
                            eprintln!("[Taurium] Context menu: open in browser {}", service_id);
                            let state = app_handle_evt.state::<WebviewState>();
                            let services = match state.services.lock() {
                                Ok(guard) => guard,
                                Err(e) => {
                                    eprintln!("[Taurium] Mutex poisoned: {}", e);
                                    return;
                                }
                            };
                            if let Some(service) = services.iter().find(|s| s.id == service_id) {
                                let _ = tauri_plugin_opener::open_url(&service.url, None::<&str>);
                            }
                        }
                        _ => {}
                    }
                }
            });

            // Hibernation timer: check every 60 seconds
            let app_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let state = app_handle.state::<WebviewState>();
                webviews::check_hibernation(&app_handle, &state);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_recipes,
            get_services,
            switch_service,
            get_last_active_service,
            save_services_cmd,
            open_settings,
            restart_app,
            reload_service,
            get_badge_counts,
            get_service_url,
            show_service_context_menu,
            get_preferences,
            save_preferences_cmd,
            apply_services,
            get_services_load_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
