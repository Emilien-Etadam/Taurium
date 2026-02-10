mod config;
mod webviews;

use config::{load_services, load_state, save_state, AppState, Service};
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

            // Register state FIRST (before creating webviews that may call invoke)
            let webview_state = WebviewState {
                created_ids: std::sync::Mutex::new(Vec::new()),
                active_id: std::sync::Mutex::new(None),
                app_data_dir,
                services: services.clone(),
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

            // Pre-create ALL service webviews during setup (hidden)
            let state = app.state::<WebviewState>();
            for service in &services {
                eprintln!("[Taurium] Pre-creating webview: {} ({})", service.id, service.url);

                let parsed_url: tauri::Url = service.url.parse().expect("Invalid service URL");
                let url = WebviewUrl::External(parsed_url);
                let builder = tauri::webview::WebviewBuilder::new(&service.id, url);

                let webview = window.add_child(
                    builder,
                    LogicalPosition::new(webviews::SIDEBAR_WIDTH, 0.0),
                    LogicalSize::new(content_width, h),
                )?;

                webview.hide()?;
                state.created_ids.lock().unwrap().push(service.id.clone());

                eprintln!("[Taurium] Webview '{}' created (hidden)", service.id);
            }

            // Listen for window resize events
            let app_handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Resized(_) = event {
                    let state = app_handle.state::<WebviewState>();
                    webviews::resize_all_webviews(&app_handle, &state);
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
