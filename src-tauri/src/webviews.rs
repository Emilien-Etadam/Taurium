use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager};

use crate::config::Service;

pub const SIDEBAR_WIDTH: f64 = 48.0;
const HIBERNATION_SECS: u64 = 600; // 10 minutes

pub struct WebviewState {
    pub created_ids: Mutex<Vec<String>>,
    pub active_id: Mutex<Option<String>>,
    pub app_data_dir: PathBuf,
    pub services: Vec<Service>,
    /// Tracks which webviews have been navigated to their real URL
    pub navigated: Mutex<HashSet<String>>,
    /// Last time each webview was actively shown
    pub last_activity: Mutex<HashMap<String, Instant>>,
    /// Badge counts per service id
    pub badge_counts: Mutex<HashMap<String, u32>>,
}

fn hide_all(app: &AppHandle, state: &WebviewState) {
    let ids = state.created_ids.lock().unwrap();
    for wv_id in ids.iter() {
        if let Some(webview) = app.get_webview(wv_id) {
            webview.hide().ok();
        }
    }
    if let Some(webview) = app.get_webview("settings") {
        webview.hide().ok();
    }
}

/// Navigate a webview to its real URL (lazy loading)
fn ensure_navigated(app: &AppHandle, state: &WebviewState, id: &str) {
    let mut navigated = state.navigated.lock().unwrap();
    if navigated.contains(id) {
        return;
    }

    // Find service URL
    if let Some(service) = state.services.iter().find(|s| s.id == id) {
        if let Some(webview) = app.get_webview(id) {
            let url = service.url.clone();
            eprintln!("[Taurium] Lazy-loading {} -> {}", id, url);
            // Use navigate or eval to go to the real URL
            let js = format!("window.location.replace('{}')", url.replace('\'', "\\'"));
            webview.eval(&js).ok();
            navigated.insert(id.to_string());
        }
    }
}

pub fn switch_to(app: &AppHandle, state: &WebviewState, id: &str) -> Result<(), String> {
    eprintln!("[Taurium] Switching to service: {}", id);
    hide_all(app, state);

    // Lazy load: navigate to real URL on first click
    ensure_navigated(app, state, id);

    let webview = app.get_webview(id).ok_or(format!("Webview '{}' not found", id))?;
    webview.show().map_err(|e| e.to_string())?;

    *state.active_id.lock().unwrap() = Some(id.to_string());

    // Update activity timestamp
    state.last_activity.lock().unwrap().insert(id.to_string(), Instant::now());

    eprintln!("[Taurium] Now showing: {}", id);
    Ok(())
}

pub fn show_settings(app: &AppHandle, state: &WebviewState) -> Result<(), String> {
    eprintln!("[Taurium] Showing settings");
    hide_all(app, state);

    let webview = app.get_webview("settings").ok_or("Settings webview not found")?;
    webview.show().map_err(|e| e.to_string())?;

    *state.active_id.lock().unwrap() = None;
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

    // Resize settings
    if let Some(settings) = app.get_webview("settings") {
        settings
            .set_size(tauri::Size::Logical(LogicalSize::new(width, height)))
            .ok();
        settings
            .set_position(tauri::Position::Logical(LogicalPosition::new(SIDEBAR_WIDTH, 0.0)))
            .ok();
    }

    // Resize all service webviews (active one first for responsiveness)
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

/// Hibernate inactive webviews to save memory
pub fn check_hibernation(app: &AppHandle, state: &WebviewState) {
    let active = state.active_id.lock().unwrap().clone();
    let mut last_activity = state.last_activity.lock().unwrap();
    let mut navigated = state.navigated.lock().unwrap();
    let now = Instant::now();

    let ids: Vec<String> = navigated.iter().cloned().collect();
    for id in ids {
        // Don't hibernate the active webview
        if active.as_deref() == Some(id.as_str()) {
            continue;
        }

        if let Some(last) = last_activity.get(&id) {
            if now.duration_since(*last).as_secs() > HIBERNATION_SECS {
                if let Some(webview) = app.get_webview(&id) {
                    eprintln!("[Taurium] Hibernating webview: {}", id);
                    webview.eval("window.location.replace('about:blank')").ok();
                    navigated.remove(&id);
                    last_activity.remove(&id);
                }
            }
        }
    }
}
