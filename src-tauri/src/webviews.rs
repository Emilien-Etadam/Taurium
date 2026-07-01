use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_plugin_notification::NotificationExt;

use crate::config::{extract_badge_count, load_preferences, Service, ServicesLoadInfo};
use crate::error::TauriumError;

/// Compact sidebar width (icons only).
pub const SIDEBAR_WIDTH: f64 = 48.0;
/// Expanded sidebar width (icons + labels + group names).
pub const SIDEBAR_EXPANDED_WIDTH: f64 = 210.0;
const HIBERNATION_SECS: u64 = 600; // 10 minutes

// Notification body templates (English)
const NOTIFY_SINGLE_FROM: &str = "1 notification from {service}";
const NOTIFY_MULTIPLE_FROM: &str = "{count} notifications from {service}";
const NOTIFY_NEW_SINGLE: &str = "New notification from {service}";
const NOTIFY_NEW_MULTIPLE: &str = "{count} new notifications from {service}";

/// Diff existing webview ids against the new service list.
/// Returns `(to_remove, to_add)`; unchanged ids are implicit (intersection).
pub(crate) fn compute_service_changes(
    old_ids: &HashSet<String>,
    new_services: &[Service],
) -> (Vec<String>, Vec<Service>) {
    let new_ids: HashSet<String> = new_services.iter().map(|s| s.id.clone()).collect();
    let to_remove: Vec<String> = old_ids.difference(&new_ids).cloned().collect();
    let to_add: Vec<Service> = new_services
        .iter()
        .filter(|s| !old_ids.contains(&s.id))
        .cloned()
        .collect();
    (to_remove, to_add)
}

/// Decide whether to notify and return the notification body, if any.
pub(crate) fn notification_body_for_badge_change(
    service_name: &str,
    count: u32,
    prev_count: u32,
    notifications_enabled: bool,
) -> Option<String> {
    if !notifications_enabled || count <= prev_count || count == 0 {
        return None;
    }
    let body = if prev_count == 0 {
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
    };
    Some(body)
}

/// Select navigated webview ids that exceeded the inactivity threshold (excluding the active one).
pub(crate) fn select_webviews_to_hibernate(
    navigated_ids: &[String],
    active_id: Option<&str>,
    last_activity: &HashMap<String, Instant>,
    now: Instant,
    hibernation_secs: u64,
) -> Vec<String> {
    navigated_ids
        .iter()
        .filter(|id| active_id != Some(id.as_str()))
        .filter(|id| {
            last_activity
                .get(*id)
                .is_some_and(|last| now.duration_since(*last).as_secs() > hibernation_secs)
        })
        .cloned()
        .collect()
}

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
    /// Current sidebar width in logical px (compact or expanded).
    pub sidebar_width: Mutex<f64>,
    /// Warnings/errors from the initial services.json load (read-only after setup).
    pub services_load_info: ServicesLoadInfo,
}

/// Current sidebar width, falling back to the compact width if the lock is poisoned.
pub fn current_sidebar_width(state: &WebviewState) -> f64 {
    state
        .sidebar_width
        .lock()
        .map(|w| *w)
        .unwrap_or(SIDEBAR_WIDTH)
}

fn is_meaningful_page_url(url: &str) -> bool {
    !url.is_empty() && url != "about:blank"
}

/// Notify the sidebar that a service webview has finished loading.
fn notify_service_loaded(app: &AppHandle, service_id: &str) {
    if let Some(sidebar) = app.get_webview("sidebar") {
        let id_json = serde_json::to_string(service_id).unwrap_or_default();
        let js = format!("window.__serviceLoaded && window.__serviceLoaded({id_json})");
        sidebar.eval(&js).ok();
    }
}

/// Handle document title change: update badge count, send notification, refresh sidebar
pub fn handle_title_change(app: &AppHandle, service_id: &str, service_name: &str, title: &str) {
    // Skip blank/empty pages (avoid unnecessary work during webview creation)
    if title.is_empty() || title == "about:blank" {
        return;
    }

    notify_service_loaded(app, service_id);

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
    if let Some(body) = notification_body_for_badge_change(
        service_name,
        count,
        prev_count,
        prefs.notifications_enabled,
    ) {
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

fn window_content_size(
    window: &tauri::Window,
    sidebar_width: f64,
) -> Result<(f64, f64), TauriumError> {
    let inner_size = window.inner_size()?;
    let scale = window.scale_factor()?;
    let width = (inner_size.width as f64 / scale) - sidebar_width;
    let height = inner_size.height as f64 / scale;
    Ok((width, height))
}

fn create_service_webview_inner(
    app: &AppHandle,
    window: &tauri::Window,
    service: &Service,
    sidebar_x: f64,
    content_width: f64,
    content_height: f64,
) -> Result<(), TauriumError> {
    let url = WebviewUrl::External("about:blank".parse().unwrap());
    let app_clone = app.clone();
    let app_for_load = app.clone();
    let sid = service.id.clone();
    let sid_for_load = service.id.clone();
    let sname = service.name.clone();
    let state = app.state::<WebviewState>();
    let data_dir = state.app_data_dir.join("webview_data").join(&service.id);
    fs::create_dir_all(&data_dir)?;

    let builder = tauri::webview::WebviewBuilder::new(&service.id, url)
        .on_page_load(move |_wv, payload| {
            if payload.event() == PageLoadEvent::Finished
                && is_meaningful_page_url(payload.url().as_str())
            {
                notify_service_loaded(&app_for_load, &sid_for_load);
            }
        })
        .on_document_title_changed(move |_wv, title| {
            handle_title_change(&app_clone, &sid, &sname, &title);
        });
    let builder = if let Some(ref ua) = service.user_agent {
        builder.user_agent(ua)
    } else {
        builder
    };
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let builder = builder.data_directory(data_dir.clone());

    let webview = window.add_child(
        builder,
        LogicalPosition::new(sidebar_x, 0.0),
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
    let sidebar_x = current_sidebar_width(&app.state::<WebviewState>());
    let (content_width, content_height) = window_content_size(&window, sidebar_x)?;

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
            sidebar_x,
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

    // Create the webview on demand if it doesn't exist yet (e.g. a service
    // added after startup). Callers of switch_to run off the main thread
    // (switch_service / apply_services are command(async)), so the add_child
    // inside create_service_webview won't re-enter and deadlock WebView2.
    if app.get_webview(id).is_none() {
        let service = {
            let services = state
                .services
                .lock()
                .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
            services.iter().find(|s| s.id == id).cloned()
        };
        let Some(service) = service else {
            return Err(TauriumError::WebviewNotFound(id.to_string()));
        };
        eprintln!("[Taurium] Webview '{}' missing, creating on demand", id);
        create_service_webview(app, &service)?;
    }

    hide_all(app, state);

    let webview = app
        .get_webview(id)
        .ok_or_else(|| TauriumError::WebviewNotFound(id.to_string()))?;

    let was_already_navigated = state
        .navigated
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .contains(id);

    // Lazy load: navigate to real URL on first click
    ensure_navigated(app, state, id);

    apply_zoom_from_state(app, state, id);

    webview.show()?;

    // Already-loaded tabs do not emit page-load/title events on re-show.
    if was_already_navigated {
        notify_service_loaded(app, id);
    }

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
    let sidebar_width = current_sidebar_width(state);
    let (width, height) = match window_content_size(&window, sidebar_width) {
        Ok(size) => size,
        Err(_) => return,
    };

    // Resize sidebar
    if let Some(sidebar) = app.get_webview("sidebar") {
        sidebar
            .set_size(tauri::Size::Logical(LogicalSize::new(
                sidebar_width,
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
                sidebar_width,
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
                    sidebar_width,
                    0.0,
                )))
                .ok();
        }
    }
}

/// Set the sidebar width (compact/expanded) and reflow all webviews accordingly.
pub fn apply_sidebar_width(app: &AppHandle, state: &WebviewState, width: f64) {
    if let Ok(mut w) = state.sidebar_width.lock() {
        *w = width;
    }
    resize_all_webviews(app, state);
}

fn cleanup_service_webview_state(state: &WebviewState, id: &str) -> Result<(), TauriumError> {
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
    Ok(())
}

fn remove_service_webview_inner(
    app: &AppHandle,
    state: &WebviewState,
    id: &str,
) -> Result<(), TauriumError> {
    if let Some(webview) = app.get_webview(id) {
        webview.hide().ok();
        webview.eval("window.location.replace('about:blank')").ok();
        webview.close()?;
    }
    cleanup_service_webview_state(state, id)?;
    eprintln!("[Taurium] Webview '{}' removed", id);
    Ok(())
}

/// Supprime une webview de service (hide → blank → close) et nettoie l'état associé.
pub fn remove_service_webview(app: &AppHandle, id: &str) -> Result<(), TauriumError> {
    let window = app.get_window("main").ok_or(TauriumError::WindowNotFound)?;
    let app_handle = app.clone();
    let id_owned = id.to_string();
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), TauriumError>>();

    window.run_on_main_thread(move || {
        let state = app_handle.state::<WebviewState>();
        let result = remove_service_webview_inner(&app_handle, &state, &id_owned);
        let _ = tx.send(result);
    })?;

    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(result) => result,
        Err(_) => Err(TauriumError::ServiceNotFound(format!(
            "Timed out removing webview '{}' on main thread",
            id
        ))),
    }
}

/// Recrée la webview d'un service (utile quand le user-agent change).
pub fn recreate_service_webview(
    app: &AppHandle,
    state: &WebviewState,
    service: &Service,
) -> Result<(), TauriumError> {
    let was_active = state
        .active_id
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .as_deref()
        == Some(service.id.as_str());

    remove_service_webview(app, &service.id)?;
    create_service_webview(app, service)?;

    if was_active {
        switch_to(app, state, &service.id)?;
    }

    Ok(())
}

/// Indique si une webview doit être recréée (user-agent modifié sur un service existant).
pub(crate) fn service_user_agent_changed(old: &Service, new: &Service) -> bool {
    old.id == new.id && old.user_agent != new.user_agent
}

/// Apply service changes: handle reorder/delete/add instantly.
pub fn apply_service_changes(
    app: &AppHandle,
    state: &WebviewState,
    new_services: Vec<Service>,
) -> Result<(), TauriumError> {
    let old_services = state
        .services
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .clone();

    let old_ids: HashSet<String> = state
        .created_ids
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .iter()
        .cloned()
        .collect();
    let (to_remove, to_add) = compute_service_changes(&old_ids, &new_services);
    let new_ids: HashSet<String> = new_services.iter().map(|s| s.id.clone()).collect();

    // Remove deleted service webviews
    for id in &to_remove {
        eprintln!("[Taurium] Removing webview: {}", id);
        remove_service_webview(app, id)?;
    }

    // Create newly added service webviews on-the-fly (best-effort: if creation
    // fails/times out here, switch_to will create it lazily on first click, so
    // don't fail the whole save).
    for service in &to_add {
        eprintln!("[Taurium] Creating new webview on-the-fly: {}", service.id);
        if let Err(e) = create_service_webview(app, service) {
            eprintln!(
                "[Taurium] On-the-fly creation of '{}' failed ({}); will create lazily on switch",
                service.id, e
            );
        }
    }

    // Recreate webviews when user-agent changed on existing services
    for service in &new_services {
        if !old_ids.contains(&service.id) {
            continue;
        }
        let Some(old) = old_services.iter().find(|s| s.id == service.id) else {
            continue;
        };
        if service_user_agent_changed(old, service) {
            eprintln!(
                "[Taurium] User-agent changed for {}, recreating webview",
                service.id
            );
            recreate_service_webview(app, state, service)?;
        }
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

    let navigated_ids: Vec<String> = navigated.iter().cloned().collect();
    let to_hibernate = select_webviews_to_hibernate(
        &navigated_ids,
        active.as_deref(),
        &last_activity,
        now,
        HIBERNATION_SECS,
    );
    for id in to_hibernate {
        if let Some(webview) = app.get_webview(&id) {
            eprintln!("[Taurium] Hibernating webview: {}", id);
            webview.eval("window.location.replace('about:blank')").ok();
            navigated.remove(&id);
            last_activity.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::time::{Duration, Instant};

    use super::{
        compute_service_changes, is_meaningful_page_url, notification_body_for_badge_change,
        select_webviews_to_hibernate, service_user_agent_changed, window_location_replace_js,
    };
    use crate::config::Service;

    #[test]
    fn test_is_meaningful_page_url() {
        assert!(!is_meaningful_page_url(""));
        assert!(!is_meaningful_page_url("about:blank"));
        assert!(is_meaningful_page_url("https://example.com"));
    }

    fn sample_service(id: &str) -> Service {
        Service {
            id: id.to_string(),
            name: format!("Service {id}"),
            url: format!("https://{id}.example.com"),
            icon: "icon.png".to_string(),
            user_agent: None,
            zoom: None,
            group: None,
        }
    }

    fn old_ids(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn assert_same_ids(actual: &[String], expected: &[&str]) {
        let mut actual_sorted = actual.to_vec();
        actual_sorted.sort();
        let mut expected_sorted: Vec<String> = expected.iter().map(|s| (*s).to_string()).collect();
        expected_sorted.sort();
        assert_eq!(actual_sorted, expected_sorted);
    }

    #[test]
    fn compute_service_changes_empty_old_adds_all() {
        let new = vec![sample_service("a"), sample_service("b")];
        let (to_remove, to_add) = compute_service_changes(&HashSet::new(), &new);
        assert!(to_remove.is_empty());
        assert_eq!(to_add.len(), 2);
        assert_eq!(to_add[0].id, "a");
        assert_eq!(to_add[1].id, "b");
    }

    #[test]
    fn compute_service_changes_unchanged_is_empty() {
        let new = vec![sample_service("a"), sample_service("b")];
        let (to_remove, to_add) = compute_service_changes(&old_ids(&["a", "b"]), &new);
        assert!(to_remove.is_empty());
        assert!(to_add.is_empty());
    }

    #[test]
    fn compute_service_changes_reorder_only_is_empty() {
        let new = vec![sample_service("b"), sample_service("a")];
        let (to_remove, to_add) = compute_service_changes(&old_ids(&["a", "b"]), &new);
        assert!(to_remove.is_empty());
        assert!(to_add.is_empty());
    }

    #[test]
    fn compute_service_changes_detects_removals() {
        let new = vec![sample_service("a")];
        let (to_remove, to_add) = compute_service_changes(&old_ids(&["a", "b", "c"]), &new);
        assert_same_ids(&to_remove, &["b", "c"]);
        assert!(to_add.is_empty());
    }

    #[test]
    fn compute_service_changes_detects_additions_preserving_order() {
        let new = vec![
            sample_service("a"),
            sample_service("b"),
            sample_service("c"),
        ];
        let (to_remove, to_add) = compute_service_changes(&old_ids(&["a"]), &new);
        assert!(to_remove.is_empty());
        assert_eq!(to_add.len(), 2);
        assert_eq!(to_add[0].id, "b");
        assert_eq!(to_add[1].id, "c");
    }

    #[test]
    fn compute_service_changes_detects_simultaneous_add_and_remove() {
        let new = vec![sample_service("b"), sample_service("d")];
        let (to_remove, to_add) = compute_service_changes(&old_ids(&["a", "b", "c"]), &new);
        assert_same_ids(&to_remove, &["a", "c"]);
        assert_eq!(to_add.len(), 1);
        assert_eq!(to_add[0].id, "d");
    }

    #[test]
    fn compute_service_changes_empty_new_removes_all() {
        let (to_remove, to_add) = compute_service_changes(&old_ids(&["a", "b"]), &[]);
        assert_same_ids(&to_remove, &["a", "b"]);
        assert!(to_add.is_empty());
    }

    #[test]
    fn notification_body_disabled_returns_none() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 5, 0, false),
            None
        );
    }

    #[test]
    fn notification_body_equal_count_returns_none() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 3, 3, true),
            None
        );
    }

    #[test]
    fn notification_body_decreased_count_returns_none() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 2, 5, true),
            None
        );
    }

    #[test]
    fn notification_body_zero_count_returns_none() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 0, 0, true),
            None
        );
        assert_eq!(
            notification_body_for_badge_change("Slack", 0, 3, true),
            None
        );
    }

    #[test]
    fn notification_body_first_single_message() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 1, 0, true),
            Some("1 notification from Slack".to_string())
        );
    }

    #[test]
    fn notification_body_first_multiple_messages() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 5, 0, true),
            Some("5 notifications from Slack".to_string())
        );
    }

    #[test]
    fn notification_body_increment_single_new_message() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 4, 3, true),
            Some("New notification from Slack".to_string())
        );
    }

    #[test]
    fn notification_body_increment_multiple_new_messages() {
        assert_eq!(
            notification_body_for_badge_change("Slack", 8, 3, true),
            Some("5 new notifications from Slack".to_string())
        );
    }

    #[test]
    fn select_webviews_to_hibernate_skips_active() {
        let now = Instant::now();
        let last_activity = HashMap::from([("active".to_string(), now - Duration::from_secs(900))]);
        let selected = select_webviews_to_hibernate(
            &["active".to_string()],
            Some("active"),
            &last_activity,
            now,
            600,
        );
        assert!(selected.is_empty());
    }

    #[test]
    fn select_webviews_to_hibernate_selects_idle_navigated() {
        let now = Instant::now();
        let last_activity = HashMap::from([("idle".to_string(), now - Duration::from_secs(601))]);
        let selected =
            select_webviews_to_hibernate(&["idle".to_string()], None, &last_activity, now, 600);
        assert_eq!(selected, vec!["idle".to_string()]);
    }

    #[test]
    fn select_webviews_to_hibernate_respects_threshold_boundary() {
        let now = Instant::now();
        let last_activity =
            HashMap::from([("borderline".to_string(), now - Duration::from_secs(600))]);
        let selected = select_webviews_to_hibernate(
            &["borderline".to_string()],
            None,
            &last_activity,
            now,
            600,
        );
        assert!(selected.is_empty());
    }

    #[test]
    fn select_webviews_to_hibernate_skips_without_activity_entry() {
        let now = Instant::now();
        let selected =
            select_webviews_to_hibernate(&["orphan".to_string()], None, &HashMap::new(), now, 600);
        assert!(selected.is_empty());
    }

    #[test]
    fn select_webviews_to_hibernate_skips_not_yet_idle() {
        let now = Instant::now();
        let last_activity = HashMap::from([("recent".to_string(), now - Duration::from_secs(30))]);
        let selected =
            select_webviews_to_hibernate(&["recent".to_string()], None, &last_activity, now, 600);
        assert!(selected.is_empty());
    }

    #[test]
    fn select_webviews_to_hibernate_multiple_candidates() {
        let now = Instant::now();
        let last_activity = HashMap::from([
            ("idle-a".to_string(), now - Duration::from_secs(700)),
            ("active".to_string(), now - Duration::from_secs(900)),
            ("idle-b".to_string(), now - Duration::from_secs(800)),
            ("recent".to_string(), now - Duration::from_secs(10)),
        ]);
        let selected = select_webviews_to_hibernate(
            &[
                "idle-a".to_string(),
                "active".to_string(),
                "idle-b".to_string(),
                "recent".to_string(),
            ],
            Some("active"),
            &last_activity,
            now,
            600,
        );
        assert_eq!(selected, vec!["idle-a".to_string(), "idle-b".to_string()]);
    }

    #[test]
    fn select_webviews_to_hibernate_ignores_non_navigated() {
        let now = Instant::now();
        let last_activity = HashMap::from([("hidden".to_string(), now - Duration::from_secs(900))]);
        let selected = select_webviews_to_hibernate(&[], None, &last_activity, now, 600);
        assert!(selected.is_empty());
    }

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

    #[test]
    fn test_service_user_agent_changed() {
        let base = Service {
            id: "svc".to_string(),
            name: "Test".to_string(),
            url: "https://example.com".to_string(),
            icon: "x".to_string(),
            user_agent: None,
            zoom: None,
            group: None,
        };
        let with_ua = Service {
            user_agent: Some("Custom".to_string()),
            ..base.clone()
        };
        assert!(!service_user_agent_changed(&base, &base));
        assert!(service_user_agent_changed(&base, &with_ua));
        assert!(service_user_agent_changed(&with_ua, &base));
    }
}
