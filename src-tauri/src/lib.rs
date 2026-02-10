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

            // Create main window
            let window = tauri::window::WindowBuilder::new(app, "main")
                .title("FerdiLight")
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
            let sidebar_webview = window.add_child(
                sidebar_builder,
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(webviews::SIDEBAR_WIDTH, h),
            )?;

            #[cfg(debug_assertions)]
            sidebar_webview.open_devtools();

            // Pre-create ALL service webviews during setup (hidden)
            // This avoids the add_child deadlock when called from command handlers
            let content_width = w - webviews::SIDEBAR_WIDTH;
            let mut created_ids = Vec::new();

            for service in &services {
                eprintln!("[FerdiLight] Pre-creating webview: {} ({})", service.id, service.url);

                let parsed_url: tauri::Url = service.url.parse().expect("Invalid service URL");
                let url = WebviewUrl::External(parsed_url);
                let builder = tauri::webview::WebviewBuilder::new(&service.id, url);

                let webview = window.add_child(
                    builder,
                    LogicalPosition::new(webviews::SIDEBAR_WIDTH, 0.0),
                    LogicalSize::new(content_width, h),
                )?;

                // Hide all webviews initially
                webview.hide()?;
                created_ids.push(service.id.clone());

                eprintln!("[FerdiLight] Webview '{}' created (hidden)", service.id);
            }

            let webview_state = WebviewState {
                created_ids: std::sync::Mutex::new(created_ids),
                active_id: std::sync::Mutex::new(None),
                app_data_dir,
                services,
            };

            app.manage(webview_state);

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
