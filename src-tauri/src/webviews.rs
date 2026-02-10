use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager};

use crate::config::Service;

pub const SIDEBAR_WIDTH: f64 = 48.0;

pub struct WebviewState {
    pub created_ids: Mutex<Vec<String>>,
    pub active_id: Mutex<Option<String>>,
    pub app_data_dir: PathBuf,
    pub services: Vec<Service>,
}

pub fn switch_to(app: &AppHandle, state: &WebviewState, id: &str) -> Result<(), String> {
    eprintln!("[FerdiLight] Switching to service: {}", id);

    // Hide all service webviews
    let ids = state.created_ids.lock().unwrap();
    for wv_id in ids.iter() {
        if let Some(webview) = app.get_webview(wv_id) {
            webview.hide().ok();
        }
    }
    drop(ids);

    // Show the requested one
    let webview = app.get_webview(id).ok_or(format!("Webview '{}' not found", id))?;
    webview.show().map_err(|e| e.to_string())?;

    *state.active_id.lock().unwrap() = Some(id.to_string());
    eprintln!("[FerdiLight] Now showing: {}", id);
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
