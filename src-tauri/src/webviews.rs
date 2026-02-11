use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_plugin_notification::NotificationExt;

use crate::config::{extract_badge_count, load_preferences, Service};

pub const SIDEBAR_WIDTH: f64 = 48.0;
const HIBERNATION_SECS: u64 = 600; // 10 minutes

pub struct WebviewState {
    pub created_ids: Mutex<Vec<String>>,
    pub active_id: Mutex<Option<String>>,
    pub app_data_dir: PathBuf,
    pub services: Mutex<Vec<Service>>,
    /// Tracks which webviews have been navigated to their real URL
    pub navigated: Mutex<HashSet<String>>,
    /// Last time each webview was actively shown
    pub last_activity: Mutex<HashMap<String, Instant>>,
    /// Badge counts per service id
    pub badge_counts: Mutex<HashMap<String, u32>>,
}

/// Handle document title change: update badge count, send notification, refresh sidebar
pub fn handle_title_change(app: &AppHandle, service_id: &str, service_name: &str, title: &str) {
    // Skip blank/empty pages (avoid unnecessary work during webview creation)
    if title.is_empty() || title == "about:blank" {
        return;
    }

    let count = extract_badge_count(title);
    eprintln!("[Taurium] Title changed: '{}' → badge count: {} (service: {})", title, count, service_id);
    let state = app.state::<WebviewState>();

    // Update badge counts (hold lock briefly, then release before eval)
    let (prev_count, badges_json) = {
        let mut badges = state.badge_counts.lock().unwrap();
        let prev = badges.get(service_id).copied().unwrap_or(0);
        if count > 0 {
            badges.insert(service_id.to_string(), count);
        } else {
            badges.remove(service_id);
        }
        let json = serde_json::to_string(&*badges).unwrap_or_default();
        (prev, json)
    }; // badge_counts lock released here

    // Send notification if badge count increased
    let prefs = load_preferences(&state.app_data_dir);
    let should_notify = prefs.notifications_enabled && count > prev_count;
    if should_notify && count > 0 {
        let body = if prev_count == 0 {
            if count == 1 {
                format!("1 notification from {}", service_name)
            } else {
                format!("{} notifications from {}", count, service_name)
            }
        } else {
            let new_msgs = count - prev_count;
            if new_msgs == 1 {
                format!("New notification from {}", service_name)
            } else {
                format!("{} new notifications from {}", new_msgs, service_name)
            }
        };
        eprintln!("[Taurium] Sending notification: {} - {}", service_name, body);
        match app.notification().builder().title(service_name).body(&body).show() {
            Ok(_) => eprintln!("[Taurium] Notification sent successfully"),
            Err(e) => eprintln!("[Taurium] Notification error: {}", e),
        }
    }

    // Update sidebar badges (lock already released, safe to eval)
    if let Some(sidebar) = app.get_webview("sidebar") {
        let js = format!("window.__updateBadges && window.__updateBadges({})", badges_json);
        sidebar.eval(&js).ok();
    }
}

/// Create a single service webview (hidden, lazy-loaded with about:blank)
pub fn create_service_webview(
    app: &AppHandle,
    window: &tauri::Window,
    service: &Service,
    content_width: f64,
    content_height: f64,
) -> Result<(), String> {
    let url = WebviewUrl::External("about:blank".parse().unwrap());
    let app_clone = app.clone();
    let sid = service.id.clone();
    let sname = service.name.clone();

    let builder = tauri::webview::WebviewBuilder::new(&service.id, url)
        .on_document_title_changed(move |_wv, title| {
            handle_title_change(&app_clone, &sid, &sname, &title);
        });

    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(SIDEBAR_WIDTH, 0.0),
            LogicalSize::new(content_width, content_height),
        )
        .map_err(|e| e.to_string())?;

    webview.hide().map_err(|e| e.to_string())?;

    let state = app.state::<WebviewState>();
    state.created_ids.lock().unwrap().push(service.id.clone());

    eprintln!("[Taurium] Webview '{}' created (hidden, lazy)", service.id);
    Ok(())
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
    let services = state.services.lock().unwrap();
    if let Some(service) = services.iter().find(|s| s.id == id) {
        if let Some(webview) = app.get_webview(id) {
            let url = service.url.clone();
            eprintln!("[Taurium] Lazy-loading {} -> {}", id, url);
            let js = format!("window.location.replace('{}')", url.replace('\'', "\\'"));
            webview.eval(&js).ok();
            navigated.insert(id.to_string());
        }
    }
}

pub fn switch_to(app: &AppHandle, state: &WebviewState, id: &str) -> Result<(), String> {
    eprintln!("[Taurium] Switching to service: {}", id);
    hide_all(app, state);

    // Webviews are only created in setup() because add_child() deadlocks
    // from command handlers on Windows. If the webview doesn't exist,
    // the service was added after startup and requires a restart.
    let webview = app.get_webview(id).ok_or_else(|| {
        format!("Service '{}' requires a restart to be available", id)
    })?;

    // Lazy load: navigate to real URL on first click
    ensure_navigated(app, state, id);

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

/// Apply service changes: handle reorder/delete instantly, flag new services for restart.
/// Returns true if restart is needed (new services were added).
pub fn apply_service_changes(app: &AppHandle, state: &WebviewState, new_services: Vec<Service>) -> Result<bool, String> {
    let old_ids: HashSet<String> = state.created_ids.lock().unwrap().iter().cloned().collect();
    let new_ids: HashSet<String> = new_services.iter().map(|s| s.id.clone()).collect();

    // Remove deleted service webviews
    for id in old_ids.difference(&new_ids) {
        eprintln!("[Taurium] Removing webview: {}", id);
        if let Some(webview) = app.get_webview(id) {
            webview.eval("window.location.replace('about:blank')").ok();
            webview.hide().ok();
        }
        state.created_ids.lock().unwrap().retain(|i| i != id);
        state.navigated.lock().unwrap().remove(id);
        state.badge_counts.lock().unwrap().remove(id);
        state.last_activity.lock().unwrap().remove(id);
    }

    // Check if new services were added (can't create webviews from command handler)
    let has_new = new_services.iter().any(|s| !old_ids.contains(&s.id));

    // Update state
    *state.services.lock().unwrap() = new_services;

    // If active service was removed, clear it
    {
        let mut active = state.active_id.lock().unwrap();
        if let Some(ref active_id) = *active {
            if !new_ids.contains(active_id) {
                *active = None;
            }
        }
    }

    // Refresh sidebar
    if let Some(sidebar) = app.get_webview("sidebar") {
        sidebar.eval("window.__reloadSidebar && window.__reloadSidebar()").ok();
    }

    if has_new {
        eprintln!("[Taurium] New services added — restart required for webview creation");
    }

    Ok(has_new)
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
