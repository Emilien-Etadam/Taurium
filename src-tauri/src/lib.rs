mod config;
mod webviews;

use config::{extract_badge_count, load_services, load_state, save_state, AppState, Service};
use std::collections::{HashMap, HashSet};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};
use webviews::WebviewState;

#[tauri::command]
fn get_services(state: tauri::State<WebviewState>) -> Vec<Service> {
    state.services.clone()
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
    if let Some(service) = state.services.iter().find(|s| s.id == id) {
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
    state.services.iter().find(|s| s.id == id).map(|s| s.url.clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
                services: services.clone(),
                navigated: std::sync::Mutex::new(HashSet::new()),
                last_activity: std::sync::Mutex::new(HashMap::new()),
                badge_counts: std::sync::Mutex::new(HashMap::new()),
            };
            app.manage(webview_state);

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

            // Pre-create ALL service webviews with about:blank (lazy loading)
            let state = app.state::<WebviewState>();
            for service in &services {
                eprintln!("[Taurium] Pre-creating webview: {} (about:blank, lazy)", service.id);

                let url = WebviewUrl::External("about:blank".parse().unwrap());
                let app_handle_badge = app.handle().clone();
                let service_id_badge = service.id.clone();
                let builder = tauri::webview::WebviewBuilder::new(&service.id, url)
                    .on_document_title_changed(move |_webview, title| {
                        let count = extract_badge_count(&title);
                        let state = app_handle_badge.state::<WebviewState>();
                        let mut badges = state.badge_counts.lock().unwrap();
                        if count > 0 {
                            badges.insert(service_id_badge.clone(), count);
                        } else {
                            badges.remove(&service_id_badge);
                        }
                        // Notify sidebar to update badges
                        if let Some(sidebar) = app_handle_badge.get_webview("sidebar") {
                            let badges_json = serde_json::to_string(&*badges).unwrap_or_default();
                            let js = format!("window.__updateBadges && window.__updateBadges({})", badges_json);
                            sidebar.eval(&js).ok();
                        }
                    });

                let webview = window.add_child(
                    builder,
                    LogicalPosition::new(webviews::SIDEBAR_WIDTH, 0.0),
                    LogicalSize::new(content_width, h),
                )?;

                webview.hide()?;
                state.created_ids.lock().unwrap().push(service.id.clone());

                eprintln!("[Taurium] Webview '{}' created (hidden, lazy)", service.id);
            }

            // Listen for window resize events
            let app_handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Resized(_) = event {
                    let state = app_handle.state::<WebviewState>();
                    webviews::resize_all_webviews(&app_handle, &state);
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
