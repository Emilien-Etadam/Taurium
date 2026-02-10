use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};

use crate::config::Service;

pub const SIDEBAR_WIDTH: f64 = 48.0;

pub struct WebviewState {
    pub created_ids: Mutex<Vec<String>>,
    pub active_id: Mutex<Option<String>>,
    pub app_data_dir: PathBuf,
    pub services: Vec<Service>,
}

pub fn create_webview(app: &AppHandle, state: &WebviewState, service: &Service) -> Result<(), String> {
    let window = app.get_window("main").ok_or("Main window not found")?;
    let inner_size = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);

    let width = (inner_size.width as f64 / scale) - SIDEBAR_WIDTH;
    let height = inner_size.height as f64 / scale;

    let _data_dir = state.app_data_dir.join("webview_data").join(&service.id);
    std::fs::create_dir_all(&_data_dir).ok();

    let parsed_url: tauri::Url = service.url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let url = WebviewUrl::External(parsed_url);

    let builder = tauri::webview::WebviewBuilder::new(&service.id, url)
        .auto_resize();

    window
        .add_child(
            builder,
            LogicalPosition::new(SIDEBAR_WIDTH, 0.0),
            LogicalSize::new(width, height),
        )
        .map_err(|e| e.to_string())?;

    state.created_ids.lock().unwrap().push(service.id.clone());
    Ok(())
}

pub fn show_webview(app: &AppHandle, id: &str) -> Result<(), String> {
    let webview = app.get_webview(id).ok_or(format!("Webview '{}' not found", id))?;
    webview.show().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn hide_all_webviews(app: &AppHandle, state: &WebviewState) {
    let ids = state.created_ids.lock().unwrap();
    for id in ids.iter() {
        if let Some(webview) = app.get_webview(id) {
            webview.hide().ok();
        }
    }
}

pub fn switch_to(app: &AppHandle, state: &WebviewState, id: &str) -> Result<(), String> {
    hide_all_webviews(app, state);

    let already_created = state.created_ids.lock().unwrap().contains(&id.to_string());

    if !already_created {
        let service = state
            .services
            .iter()
            .find(|s| s.id == id)
            .ok_or(format!("Service '{}' not found", id))?
            .clone();
        create_webview(app, state, &service)?;
    } else {
        show_webview(app, id)?;
    }

    *state.active_id.lock().unwrap() = Some(id.to_string());
    Ok(())
}

pub fn resize_all_webviews(app: &AppHandle, state: &WebviewState) {
    let window = match app.get_window("main") {
        Some(w) => w,
        None => return,
    };
    let inner_size = match window.inner_size() {
        Ok(s) => s,
        Err(_) => return,
    };
    let scale = window.scale_factor().unwrap_or(1.0);

    let width = (inner_size.width as f64 / scale) - SIDEBAR_WIDTH;
    let height = inner_size.height as f64 / scale;

    // Resize sidebar
    if let Some(sidebar) = app.get_webview("sidebar") {
        sidebar
            .set_size(tauri::Size::Logical(LogicalSize::new(SIDEBAR_WIDTH, height)))
            .ok();
    }

    // Resize service webviews
    let ids = state.created_ids.lock().unwrap();
    for id in ids.iter() {
        if let Some(webview) = app.get_webview(id) {
            webview
                .set_size(tauri::Size::Logical(LogicalSize::new(width, height)))
                .ok();
            webview
                .set_position(tauri::Position::Logical(LogicalPosition::new(SIDEBAR_WIDTH, 0.0)))
                .ok();
        }
    }
}
