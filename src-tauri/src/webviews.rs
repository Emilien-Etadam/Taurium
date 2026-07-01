use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_plugin_notification::NotificationExt;

use crate::config::{extract_badge_count, load_preferences, Service};
use crate::error::TauriumError;

pub const SIDEBAR_WIDTH: f64 = 48.0;
const HIBERNATION_SECS: u64 = 600; // 10 minutes

// Notification body templates (English)
const NOTIFY_SINGLE_FROM: &str = "1 notification from {service}";
const NOTIFY_MULTIPLE_FROM: &str = "{count} notifications from {service}";
const NOTIFY_NEW_SINGLE: &str = "New notification from {service}";
const NOTIFY_NEW_MULTIPLE: &str = "{count} new notifications from {service}";

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
    eprintln!(
        "[Taurium] Title changed: '{}' → badge count: {} (service: {})",
        title, count, service_id
    );
    let state = app.state::<WebviewState>();

    // Update badge counts (hold lock briefly, then release before eval)
    let (prev_count, badges_json) = {
        let mut badges = match state.badge_counts.lock() {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("[Taurium] Mutex poisoned: {}", e);
                return;
            }
        };
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
        let body = notification_body(prev_count, count, service_name);
        eprintln!(
            "[Taurium] Sending notification: {} - {}",
            service_name, body
        );
        match app
            .notification()
            .builder()
            .title(service_name)
            .body(&body)
            .show()
        {
            Ok(_) => eprintln!("[Taurium] Notification sent successfully"),
            Err(e) => eprintln!("[Taurium] Notification error: {}", e),
        }
    }

    // Update sidebar badges (lock already released, safe to eval)
    if let Some(sidebar) = app.get_webview("sidebar") {
        let js = format!(
            "window.__updateBadges && window.__updateBadges({})",
            badges_json
        );
        sidebar.eval(&js).ok();
    }

    // Per-service zoom once the remote document is live (title updates after load)
    let zoom = {
        let services = match state.services.lock() {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("[Taurium] Mutex poisoned: {}", e);
                return;
            }
        };
        services
            .iter()
            .find(|s| s.id == service_id)
            .and_then(|s| s.zoom)
    };
    if let Some(wv) = app.get_webview(service_id) {
        apply_service_body_zoom(&wv, zoom);
    }
}

fn notification_body(prev_count: u32, count: u32, service_name: &str) -> String {
    if prev_count == 0 {
        if count == 1 {
            NOTIFY_SINGLE_FROM.replace("{service}", service_name)
        } else {
            NOTIFY_MULTIPLE_FROM
                .replace("{count}", &count.to_string())
                .replace("{service}", service_name)
        }
    } else {
        let new_msgs = count - prev_count;
        if new_msgs == 1 {
            NOTIFY_NEW_SINGLE.replace("{service}", service_name)
        } else {
            NOTIFY_NEW_MULTIPLE
                .replace("{count}", &new_msgs.to_string())
                .replace("{service}", service_name)
        }
    }
}

/// Reload a service webview by navigating it back to its configured URL.
pub fn reload_service_webview(
    app: &AppHandle,
    state: &WebviewState,
    id: &str,
) -> Result<(), TauriumError> {
    eprintln!("[Taurium] Reloading service: {}", id);
    let services = state
        .services
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
    if let Some(service) = services.iter().find(|s| s.id == id) {
        if let Some(webview) = app.get_webview(id) {
            let url = service.url.clone();
            let js = window_location_replace_js(&url);
            webview.eval(&js)?;
        }
    }
    Ok(())
}

fn window_content_size(window: &tauri::Window) -> Result<(f64, f64), TauriumError> {
    let inner_size = window.inner_size()?;
    let scale = window.scale_factor()?;
    let width = (inner_size.width as f64 / scale) - SIDEBAR_WIDTH;
    let height = inner_size.height as f64 / scale;
    Ok((width, height))
}

fn create_service_webview_inner(
    app: &AppHandle,
    window: &tauri::Window,
    service: &Service,
    content_width: f64,
    content_height: f64,
) -> Result<(), TauriumError> {
    let url = WebviewUrl::External("about:blank".parse().unwrap());
    let app_clone = app.clone();
    let sid = service.id.clone();
    let sname = service.name.clone();
    let state = app.state::<WebviewState>();
    let data_dir = state.app_data_dir.join("webview_data").join(&service.id);
    fs::create_dir_all(&data_dir)?;

    let builder = tauri::webview::WebviewBuilder::new(&service.id, url).on_document_title_changed(
        move |_wv, title| {
            handle_title_change(&app_clone, &sid, &sname, &title);
        },
    );
    let builder = if let Some(ref ua) = service.user_agent {
        builder.user_agent(ua)
    } else {
        builder
    };
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let builder = builder.data_directory(data_dir.clone());

    let webview = window.add_child(
        builder,
        LogicalPosition::new(SIDEBAR_WIDTH, 0.0),
        LogicalSize::new(content_width, content_height),
    )?;

    webview.hide()?;

    state
        .created_ids
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .push(service.id.clone());

    eprintln!("[Taurium] Webview '{}' created (hidden, lazy)", service.id);
    Ok(())
}

/// Create a single service webview (hidden, lazy-loaded with about:blank).
/// Safe to call from command handlers: it posts add_child() on the main thread.
pub fn create_service_webview(app: &AppHandle, service: &Service) -> Result<(), TauriumError> {
    let window = app.get_window("main").ok_or(TauriumError::WindowNotFound)?;
    let (content_width, content_height) = window_content_size(&window)?;

    let app_handle = app.clone();
    let window_handle = window.clone();
    let service_cloned = service.clone();
    let service_id = service.id.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), TauriumError>>();

    window.run_on_main_thread(move || {
        let result = create_service_webview_inner(
            &app_handle,
            &window_handle,
            &service_cloned,
            content_width,
            content_height,
        );
        let _ = tx.send(result);
    })?;

    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(result) => result,
        Err(_) => Err(TauriumError::ServiceNotFound(format!(
            "Timed out creating webview '{}' on main thread",
            service_id
        ))),
    }
}

fn hide_all(app: &AppHandle, state: &WebviewState) {
    let ids = match state.created_ids.lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
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
    let mut navigated = match state.navigated.lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
    if navigated.contains(id) {
        return;
    }

    // Find service URL
    let services = match state.services.lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
    if let Some(service) = services.iter().find(|s| s.id == id) {
        if let Some(webview) = app.get_webview(id) {
            let url = service.url.clone();
            eprintln!("[Taurium] Lazy-loading {} -> {}", id, url);
            let js = window_location_replace_js(&url);
            webview.eval(&js).ok();
            navigated.insert(id.to_string());
        }
    }
}

pub(crate) fn window_location_replace_js(url: &str) -> String {
    let safe_url = serde_json::to_string(url).unwrap_or_else(|_| "\"about:blank\"".to_string());
    format!("window.location.replace({})", safe_url)
}

/// Applies `document.body.style.zoom` for this webview (`None` / 1.0 clears zoom).
pub(crate) fn apply_service_body_zoom(webview: &tauri::Webview, zoom: Option<f64>) {
    let z = zoom.filter(|v| v.is_finite()).unwrap_or(1.0);
    let js = if (z - 1.0).abs() < f64::EPSILON {
        r#"try{if(document.body)document.body.style.zoom="";}catch(e){}"#.to_string()
    } else {
        let literal =
            serde_json::to_string(&format!("{z}")).unwrap_or_else(|_| "\"1\"".to_string());
        format!("try{{if(document.body)document.body.style.zoom={literal};}}catch(e){{}}")
    };
    webview.eval(&js).ok();
}

/// Re-read zoom from state and apply to the service webview (e.g. after tab switch).
pub fn apply_zoom_from_state(app: &AppHandle, state: &WebviewState, id: &str) {
    let zoom = {
        let services = match state.services.lock() {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("[Taurium] Mutex poisoned: {}", e);
                return;
            }
        };
        services.iter().find(|s| s.id == id).and_then(|s| s.zoom)
    };
    if let Some(webview) = app.get_webview(id) {
        apply_service_body_zoom(&webview, zoom);
    }
}

pub fn switch_to(app: &AppHandle, state: &WebviewState, id: &str) -> Result<(), TauriumError> {
    eprintln!("[Taurium] Switching to service: {}", id);
    hide_all(app, state);

    // Webviews are only created in setup() because add_child() deadlocks
    // from command handlers on Windows. If the webview doesn't exist,
    // the service was added after startup and requires a restart.
    let webview = app
        .get_webview(id)
        .ok_or_else(|| TauriumError::WebviewNotFound(id.to_string()))?;

    // Lazy load: navigate to real URL on first click
    ensure_navigated(app, state, id);

    apply_zoom_from_state(app, state, id);

    webview.show()?;

    *state
        .active_id
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))? = Some(id.to_string());

    // Update activity timestamp
    state
        .last_activity
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .insert(id.to_string(), Instant::now());

    eprintln!("[Taurium] Now showing: {}", id);
    Ok(())
}

pub fn show_settings(app: &AppHandle, state: &WebviewState) -> Result<(), TauriumError> {
    eprintln!("[Taurium] Showing settings");
    hide_all(app, state);

    let webview = app
        .get_webview("settings")
        .ok_or_else(|| TauriumError::WebviewNotFound("settings".to_string()))?;
    webview.show()?;

    *state
        .active_id
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))? = None;
    Ok(())
}

pub fn resize_all_webviews(app: &AppHandle, state: &WebviewState) {
    let window = match app.get_window("main") {
        Some(w) => w,
        None => return,
    };
    let (width, height) = match window_content_size(&window) {
        Ok(size) => size,
        Err(_) => return,
    };

    // Resize sidebar
    if let Some(sidebar) = app.get_webview("sidebar") {
        sidebar
            .set_size(tauri::Size::Logical(LogicalSize::new(
                SIDEBAR_WIDTH,
                height,
            )))
            .ok();
    }

    // Resize settings
    if let Some(settings) = app.get_webview("settings") {
        settings
            .set_size(tauri::Size::Logical(LogicalSize::new(width, height)))
            .ok();
        settings
            .set_position(tauri::Position::Logical(LogicalPosition::new(
                SIDEBAR_WIDTH,
                0.0,
            )))
            .ok();
    }

    // Resize all service webviews (active one first for responsiveness)
    let ids = match state.created_ids.lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
    for id in ids.iter() {
        if let Some(webview) = app.get_webview(id) {
            webview
                .set_size(tauri::Size::Logical(LogicalSize::new(width, height)))
                .ok();
            webview
                .set_position(tauri::Position::Logical(LogicalPosition::new(
                    SIDEBAR_WIDTH,
                    0.0,
                )))
                .ok();
        }
    }
}

/// Apply service changes: handle reorder/delete/add instantly.
pub fn apply_service_changes(
    app: &AppHandle,
    state: &WebviewState,
    new_services: Vec<Service>,
) -> Result<(), TauriumError> {
    let old_ids: HashSet<String> = state
        .created_ids
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .iter()
        .cloned()
        .collect();
    let new_ids: HashSet<String> = new_services.iter().map(|s| s.id.clone()).collect();

    // Remove deleted service webviews
    for id in old_ids.difference(&new_ids) {
        eprintln!("[Taurium] Removing webview: {}", id);
        if let Some(webview) = app.get_webview(id) {
            webview.eval("window.location.replace('about:blank')").ok();
            webview.hide().ok();
        }
        state
            .created_ids
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
            .retain(|i| i != id);
        state
            .navigated
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
            .remove(id);
        state
            .badge_counts
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
            .remove(id);
        state
            .last_activity
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
            .remove(id);
    }

    // Create newly added service webviews on-the-fly
    for service in new_services.iter().filter(|s| !old_ids.contains(&s.id)) {
        eprintln!("[Taurium] Creating new webview on-the-fly: {}", service.id);
        create_service_webview(app, service)?;
    }

    // Update state
    *state
        .services
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))? = new_services;

    // If active service was removed, clear it
    {
        let mut active = state
            .active_id
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
        if let Some(ref active_id) = *active {
            if !new_ids.contains(active_id) {
                *active = None;
            }
        }
    }

    // Refresh sidebar
    if let Some(sidebar) = app.get_webview("sidebar") {
        sidebar
            .eval("window.__reloadSidebar && window.__reloadSidebar()")
            .ok();
    }

    Ok(())
}

/// Hibernate inactive webviews to save memory
pub fn check_hibernation(app: &AppHandle, state: &WebviewState) {
    let active = match state.active_id.lock() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
    let mut last_activity = match state.last_activity.lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
    let mut navigated = match state.navigated.lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
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

#[cfg(test)]
mod tests {
    use super::window_location_replace_js;

    #[test]
    fn test_url_escaping_in_window_location_replace_js() {
        let urls = [
            "https://example.com/path/it's-here",
            r#"https://example.com/?q="quoted""#,
            "https://example.com/with spaces/here",
            "https://example.com/cafe/été/你好",
        ];

        for original in urls {
            let js = window_location_replace_js(original);
            assert!(js.starts_with("window.location.replace("));
            assert!(js.ends_with(')'));

            let prefix = "window.location.replace(";
            let inner = &js[prefix.len()..js.len() - 1];
            let decoded: String =
                serde_json::from_str(inner).expect("escaped URL should stay valid JSON string");
            assert_eq!(decoded, original);
        }
    }
}
