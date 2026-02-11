mod config;
mod webviews;

use config::{load_preferences, load_services, load_state, save_state, AppState, Preferences, Service};
use std::collections::{HashMap, HashSet};
use tauri::menu::{ContextMenu, MenuBuilder, MenuItemBuilder};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};
use webviews::WebviewState;

#[tauri::command]
fn get_services(state: tauri::State<WebviewState>) -> Vec<Service> {
    state.services.lock().unwrap().clone()
}

#[tauri::command]
fn switch_service(app: tauri::AppHandle, state: tauri::State<WebviewState>, id: String) -> Result<(), String> {
    webviews::switch_to(&app, &state, &id)?;

    let app_state = AppState {
        last_active_service: Some(id),
    };
    save_state(&state.app_data_dir, &app_state);

    Ok(())
}

#[tauri::command]
fn get_last_active_service(state: tauri::State<WebviewState>) -> Option<String> {
    let app_state = load_state(&state.app_data_dir);
    app_state.last_active_service
}

#[tauri::command]
fn save_services(state: tauri::State<WebviewState>, services: Vec<Service>) -> Result<(), String> {
    config::save_services(&state.app_data_dir, &services);
    eprintln!("[Taurium] Services saved ({} services)", services.len());
    Ok(())
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle, state: tauri::State<WebviewState>) -> Result<(), String> {
    webviews::show_settings(&app, &state)
}

#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    eprintln!("[Taurium] Restarting app...");
    tauri::process::restart(&app.env());
}

#[tauri::command]
fn reload_service(app: tauri::AppHandle, state: tauri::State<WebviewState>, id: String) -> Result<(), String> {
    eprintln!("[Taurium] Reloading service: {}", id);
    let services = state.services.lock().unwrap();
    if let Some(service) = services.iter().find(|s| s.id == id) {
        if let Some(webview) = app.get_webview(&id) {
            let url = service.url.clone();
            let js = format!("window.location.replace('{}')", url.replace('\'', "\\'"));
            webview.eval(&js).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn get_badge_counts(state: tauri::State<WebviewState>) -> HashMap<String, u32> {
    state.badge_counts.lock().unwrap().clone()
}

#[tauri::command]
fn get_service_url(state: tauri::State<WebviewState>, id: String) -> Option<String> {
    state.services.lock().unwrap().iter().find(|s| s.id == id).map(|s| s.url.clone())
}

#[tauri::command]
fn apply_services(app: tauri::AppHandle, state: tauri::State<WebviewState>) -> Result<bool, String> {
    let new_services = load_services(&state.app_data_dir);
    webviews::apply_service_changes(&app, &state, new_services)
}

#[tauri::command]
fn show_service_context_menu(app: tauri::AppHandle, id: String) -> Result<(), String> {
    eprintln!("[Taurium] Showing context menu for: {}", id);

    // Store the target service id for the menu event handler
    *app.state::<ContextMenuTarget>().0.lock().unwrap() = Some(id);

    let reload_item = MenuItemBuilder::with_id("ctx_reload", "Reload")
        .build(&app)
        .map_err(|e| e.to_string())?;
    let open_item = MenuItemBuilder::with_id("ctx_open_browser", "Open in browser")
        .build(&app)
        .map_err(|e| e.to_string())?;

    let menu = MenuBuilder::new(&app)
        .item(&reload_item)
        .item(&open_item)
        .build()
        .map_err(|e| e.to_string())?;

    let window = app.get_window("main").ok_or("Window not found")?;
    menu.popup(window).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn get_preferences(state: tauri::State<WebviewState>) -> Preferences {
    load_preferences(&state.app_data_dir)
}

#[tauri::command]
fn save_preferences_cmd(state: tauri::State<WebviewState>, prefs: Preferences) -> Result<(), String> {
    config::save_preferences(&state.app_data_dir, &prefs);
    eprintln!("[Taurium] Preferences saved");
    Ok(())
}

// Holds the service ID targeted by the context menu
pub struct ContextMenuTarget(std::sync::Mutex<Option<String>>);

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

            let services = load_services(&app_data_dir);

            // Register state FIRST
            let webview_state = WebviewState {
                created_ids: std::sync::Mutex::new(Vec::new()),
                active_id: std::sync::Mutex::new(None),
                app_data_dir,
                services: std::sync::Mutex::new(services.clone()),
                navigated: std::sync::Mutex::new(HashSet::new()),
                last_activity: std::sync::Mutex::new(HashMap::new()),
                badge_counts: std::sync::Mutex::new(HashMap::new()),
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
            );
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
            );
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
                eprintln!("[Taurium] Pre-creating webview: {} (about:blank, lazy)", service.id);
                webviews::create_service_webview(
                    app.handle(),
                    &window,
                    service,
                    content_width,
                    h,
                )?;
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
                let target_id = app_handle_evt.state::<ContextMenuTarget>().0.lock().unwrap().clone();

                if let Some(service_id) = target_id {
                    match menu_id {
                        "ctx_reload" => {
                            eprintln!("[Taurium] Context menu: reload {}", service_id);
                            let state = app_handle_evt.state::<WebviewState>();
                            let services = state.services.lock().unwrap();
                            if let Some(service) = services.iter().find(|s| s.id == service_id) {
                                if let Some(webview) = app_handle_evt.get_webview(&service_id) {
                                    let url = service.url.clone();
                                    let js = format!("window.location.replace('{}')", url.replace('\'', "\\'"));
                                    webview.eval(&js).ok();
                                }
                            }
                        }
                        "ctx_open_browser" => {
                            eprintln!("[Taurium] Context menu: open in browser {}", service_id);
                            let state = app_handle_evt.state::<WebviewState>();
                            let services = state.services.lock().unwrap();
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
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    let state = app_handle.state::<WebviewState>();
                    webviews::check_hibernation(&app_handle, &state);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_services,
            switch_service,
            get_last_active_service,
            save_services,
            open_settings,
            restart_app,
            reload_service,
            get_badge_counts,
            get_service_url,
            show_service_context_menu,
            get_preferences,
            save_preferences_cmd,
            apply_services,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
