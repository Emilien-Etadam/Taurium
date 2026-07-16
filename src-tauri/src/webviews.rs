use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};
use tauri_plugin_notification::NotificationExt;

use crate::config::{
    extract_badge_count, load_preferences, Service, ServicesLoadInfo, NOTIFY_ALL, NOTIFY_OFF,
};
use crate::error::TauriumError;

/// Minimum sidebar width / fallback (icons only). The actual width is driven by
/// the frontend (it depends on icon size and the expanded state).
pub const SIDEBAR_WIDTH: f64 = 48.0;

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

/// Reflect the total unread count on the app's taskbar icon.
///
/// Windows has no numeric taskbar badge, so an overlay dot is shown while there
/// is any unread; other desktops use the native badge count (a number on docks
/// that support it, e.g. Unity). Cleared when the total drops to zero.
pub fn update_taskbar_indicator(app: &AppHandle, total: u32) {
    let Some(window) = app.get_window("main") else {
        return;
    };

    #[cfg(target_os = "windows")]
    {
        let overlay = if total > 0 {
            Some(unread_overlay_icon())
        } else {
            None
        };
        if let Err(e) = window.set_overlay_icon(overlay) {
            eprintln!("[Taurium] set_overlay_icon failed: {e}");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let count = if total > 0 { Some(total as i64) } else { None };
        if let Err(e) = window.set_badge_count(count) {
            eprintln!("[Taurium] set_badge_count failed: {e}");
        }
    }
}

/// Briefly flash / highlight the taskbar entry to signal a new notification.
/// No-op on the focused window on most platforms.
fn flash_taskbar(app: &AppHandle) {
    if let Some(window) = app.get_window("main") {
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
    }
}

/// A small red disc used as the Windows taskbar overlay when there is unread.
/// Built once (32×32 RGBA, anti-aliased edge) and reused.
#[cfg(target_os = "windows")]
fn unread_overlay_icon() -> tauri::image::Image<'static> {
    use std::sync::OnceLock;
    static PIXELS: OnceLock<Vec<u8>> = OnceLock::new();
    const SIZE: u32 = 32;
    let rgba = PIXELS.get_or_init(|| {
        let mut buf = vec![0u8; (SIZE * SIZE * 4) as usize];
        let radius = SIZE as f32 / 2.0;
        let center = radius - 0.5;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                // Anti-aliased edge over the outermost pixel.
                let alpha = ((radius - dist).clamp(0.0, 1.0) * 255.0) as u8;
                let idx = ((y * SIZE + x) * 4) as usize;
                buf[idx] = 0xE5; // R
                buf[idx + 1] = 0x3E; // G
                buf[idx + 2] = 0x3E; // B
                buf[idx + 3] = alpha;
            }
        }
        buf
    });
    tauri::image::Image::new(rgba, SIZE, SIZE)
}

/// Clear now-muted services from the badge map and return the new taskbar total.
/// Locks are taken sequentially (never nested) to avoid deadlocks.
fn refresh_badges_for_levels(state: &WebviewState) -> u32 {
    let off_ids: Vec<String> = match state.services.lock() {
        Ok(services) => services
            .iter()
            .filter(|s| s.notify_level() == NOTIFY_OFF)
            .map(|s| s.id.clone())
            .collect(),
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return 0;
        }
    };
    match state.badge_counts.lock() {
        Ok(mut badges) => {
            for id in &off_ids {
                badges.remove(id);
            }
            badges.values().copied().sum()
        }
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            0
        }
    }
}

/// Drop ids belonging to "keep alive" services from hibernation candidates.
pub(crate) fn filter_hibernation_candidates(
    navigated_ids: &HashSet<String>,
    keep_alive_ids: &HashSet<String>,
) -> Vec<String> {
    navigated_ids
        .iter()
        .filter(|id| !keep_alive_ids.contains(*id))
        .cloned()
        .collect()
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

    let state = app.state::<WebviewState>();

    // Per-service notification level: "all" (notify + badge), "badge" (silent
    // unread badge) or "off" (fully muted). Absent/unknown falls back to "all".
    let level = match state.services.lock() {
        Ok(services) => services
            .iter()
            .find(|s| s.id == service_id)
            .map(|s| s.notify_level())
            .unwrap_or(NOTIFY_ALL),
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };

    // A muted service keeps no badge and is excluded from the taskbar total:
    // forcing the count to 0 removes it from the badge map below.
    let count = if level == NOTIFY_OFF {
        0
    } else {
        extract_badge_count(title)
    };
    eprintln!(
        "[Taurium] Title changed: '{}' → badge count: {} (service: {}, notify: {})",
        title, count, service_id, level
    );

    // Update badge counts and compute the taskbar total (hold lock briefly).
    let (prev_count, badges_json, total) = {
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
        let total: u32 = badges.values().copied().sum();
        let json = serde_json::to_string(&*badges).unwrap_or_default();
        (prev, json, total)
    }; // badge_counts lock released here

    // Desktop notification: only for "all" services (and when the global switch
    // is on). "badge" services increment the badge silently.
    let prefs = load_preferences(&state.app_data_dir);
    let notify_allowed = prefs.notifications_enabled && level == NOTIFY_ALL;
    if let Some(body) =
        notification_body_for_badge_change(service_name, count, prev_count, notify_allowed)
    {
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
        // Draw attention by briefly flashing the taskbar entry.
        flash_taskbar(app);
    }

    // Update sidebar badges (lock already released, safe to eval)
    if let Some(sidebar) = app.get_webview("sidebar") {
        let js = format!(
            "window.__updateBadges && window.__updateBadges({})",
            badges_json
        );
        sidebar.eval(&js).ok();
    }

    // Reflect the total unread count on the app's taskbar icon.
    update_taskbar_indicator(app, total);

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

/// Where a `window.open()` request coming from a service should be routed.
#[derive(Debug, PartialEq)]
pub(crate) enum PopupTarget {
    /// Navigate the service's own webview to the URL, like a browser tab
    /// (auth/SSO flows, account switching, same-site pop-outs).
    SameView,
    /// Open as a real popup window sharing the service's session. Reserved
    /// for scripted popups (`about:blank`): their content is injected by the
    /// opener, so they cannot be navigated in place.
    PopupWindow,
    /// Hand off to the system browser (regular external links).
    SystemBrowser,
}

/// SSO / login hosts that must stay in-app: the auth flow needs the
/// service's cookies to complete.
const IN_APP_POPUP_HOSTS: &[&str] = &[
    "login.microsoftonline.com",
    "login.live.com",
    "login.microsoft.com",
    "login.windows.net",
    "account.live.com",
    "account.microsoft.com",
    "teams.live.com",
    "teams.microsoft.com",
    "accounts.google.com",
    "appleid.apple.com",
    "id.atlassian.com",
];

/// Identity-provider domain suffixes (the tenant subdomain varies).
const IN_APP_POPUP_HOST_SUFFIXES: &[&str] = &[
    ".okta.com",
    ".auth0.com",
    ".onelogin.com",
    ".duosecurity.com",
    ".b2clogin.com",
];

/// Last two labels of a host — a crude registrable-domain approximation,
/// good enough for the service catalog (no `co.uk`-style entries there).
fn host_site(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        host.to_string()
    } else {
        labels[labels.len() - 2..].join(".")
    }
}

pub(crate) fn classify_popup_url(url: &Url, service_host: &str) -> PopupTarget {
    // Non-http popups (about:blank…) are scripted by the opener and only
    // work as a real window.
    if url.scheme() != "http" && url.scheme() != "https" {
        return PopupTarget::PopupWindow;
    }
    let Some(host) = url.host_str() else {
        return PopupTarget::PopupWindow;
    };
    let site = host_site(service_host);
    if !site.is_empty() && (host == site || host.ends_with(&format!(".{site}"))) {
        return PopupTarget::SameView;
    }
    if IN_APP_POPUP_HOSTS.contains(&host)
        || IN_APP_POPUP_HOST_SUFFIXES.iter().any(|s| host.ends_with(s))
    {
        return PopupTarget::SameView;
    }
    PopupTarget::SystemBrowser
}

static POPUP_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Handle a `window.open()` request from a service webview. Without this
/// handler Tauri drops the request entirely (broken `target="_blank"` links,
/// broken OAuth/account-switch popups). Auth flows and same-site pop-outs
/// navigate the service's own webview in place, so the user stays in the
/// same window; scripted popups get a real window sharing the service's
/// session (`window_features` wires the WebView2 environment on Windows and
/// the related view on Linux).
fn handle_new_window(
    app: &AppHandle,
    service_id: &str,
    service_host: &str,
    user_agent: Option<&str>,
    url: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    match classify_popup_url(&url, service_host) {
        PopupTarget::SystemBrowser => {
            eprintln!("[Taurium] Popup from '{service_id}' -> system browser: {url}");
            if let Err(e) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>) {
                eprintln!("[Taurium] Failed to open '{url}' in browser: {e}");
            }
            NewWindowResponse::Deny
        }
        PopupTarget::SameView => {
            eprintln!("[Taurium] Popup from '{service_id}' -> same view: {url}");
            if let Some(webview) = app.get_webview(service_id) {
                match webview.navigate(url.clone()) {
                    Ok(()) => return NewWindowResponse::Deny,
                    Err(e) => {
                        // Fall back to a real popup window below.
                        eprintln!("[Taurium] In-place navigation failed for '{service_id}': {e}");
                    }
                }
            }
            create_popup_window(app, service_id, user_agent, url, features)
        }
        PopupTarget::PopupWindow => create_popup_window(app, service_id, user_agent, url, features),
    }
}

/// Open `url` as a real popup window sharing the opener's session.
fn create_popup_window(
    app: &AppHandle,
    service_id: &str,
    user_agent: Option<&str>,
    url: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    let label = format!(
        "{}-popup-{}",
        service_id,
        POPUP_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    eprintln!("[Taurium] Popup from '{service_id}' -> in-app window '{label}': {url}");
    let mut builder = tauri::WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(service_id)
        .inner_size(900.0, 700.0)
        .window_features(features)
        .on_document_title_changed(|window, title| {
            let _ = window.set_title(&title);
        });
    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }
    match builder.build() {
        Ok(window) => NewWindowResponse::Create { window },
        Err(e) => {
            eprintln!("[Taurium] Failed to create popup window '{label}': {e}");
            NewWindowResponse::Deny
        }
    }
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

    // Idempotency guard. `app.get_webview(id)` — checked off the main thread
    // in switch_to — can briefly return None even though the label is already
    // registered (e.g. two rapid switches racing a creation in flight).
    // Without this, switch_to would call add_child a second time and fail with
    // "a webview with label `…` already exists" (surfaced as a toast). Here we
    // run on the main thread, where get_webview is authoritative: if the
    // webview already exists, just reconcile our bookkeeping and skip
    // re-creation.
    if app.get_webview(&service.id).is_some() {
        let mut created = state
            .created_ids
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?;
        if !created.contains(&service.id) {
            created.push(service.id.clone());
        }
        eprintln!(
            "[Taurium] Webview '{}' already exists, skipping creation",
            service.id
        );
        return Ok(());
    }

    let data_dir = state.app_data_dir.join("webview_data").join(&service.id);
    fs::create_dir_all(&data_dir)?;

    // Les services embarqués (Slack, etc.) utilisent souvent l’API HTML5
    // drag-and-drop ; le handler natif Tauri bloque ces événements DOM.
    let builder = tauri::webview::WebviewBuilder::new(&service.id, url)
        .disable_drag_drop_handler()
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
    let app_for_popup = app.clone();
    let sid_for_popup = service.id.clone();
    let ua_for_popup = service.user_agent.clone();
    let service_host = Url::parse(&service.url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let builder = builder.on_new_window(move |url, features| {
        handle_new_window(
            &app_for_popup,
            &sid_for_popup,
            &service_host,
            ua_for_popup.as_deref(),
            url,
            features,
        )
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

/// Hint the browser engine about how aggressively this webview should hold on
/// to memory. On Windows, `low` puts WebView2 in `MemoryUsageTargetLevel::Low`
/// — it sheds caches and GCs aggressively WITHOUT pausing script, so hidden
/// services keep emitting title/badge updates. `Normal` restores the default
/// for the visible service. No-op on other platforms (WebKitGTK has no
/// equivalent runtime knob).
fn set_memory_usage_target(webview: &tauri::Webview, low: bool) {
    #[cfg(target_os = "windows")]
    {
        let result = webview.with_webview(move |platform_webview| {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            use windows_core::Interface;

            let level = if low {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            };
            unsafe {
                let core = match platform_webview.controller().CoreWebView2() {
                    Ok(core) => core,
                    Err(e) => {
                        eprintln!("[Taurium] CoreWebView2 unavailable: {e}");
                        return;
                    }
                };
                // Older WebView2 runtimes may not implement ICoreWebView2_19;
                // the hint is best-effort.
                if let Ok(wv) = core.cast::<ICoreWebView2_19>() {
                    if let Err(e) = wv.SetMemoryUsageTargetLevel(level) {
                        eprintln!("[Taurium] SetMemoryUsageTargetLevel failed: {e}");
                    }
                }
            }
        });
        if let Err(e) = result {
            eprintln!("[Taurium] with_webview failed: {e}");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (webview, low);
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
            set_memory_usage_target(&webview, true);
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
    set_memory_usage_target(&webview, false);

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

fn cleanup_service_webview_state(
    state: &WebviewState,
    id: &str,
    keep_badge: bool,
) -> Result<(), TauriumError> {
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
    if !keep_badge {
        state
            .badge_counts
            .lock()
            .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
            .remove(id);
    }
    state
        .last_activity
        .lock()
        .map_err(|e| TauriumError::MutexPoisoned(e.to_string()))?
        .remove(id);
    Ok(())
}

fn close_service_webview_inner(
    app: &AppHandle,
    state: &WebviewState,
    id: &str,
    keep_badge: bool,
) -> Result<(), TauriumError> {
    if let Some(webview) = app.get_webview(id) {
        webview.hide().ok();
        webview.eval("window.location.replace('about:blank')").ok();
        webview.close()?;
    }
    cleanup_service_webview_state(state, id, keep_badge)?;
    eprintln!("[Taurium] Webview '{}' closed", id);
    Ok(())
}

/// Ferme une webview de service (hide → blank → close) et nettoie l'état associé.
/// Fermer (plutôt que naviguer vers about:blank) libère tout l'arbre de
/// processus WebView2/WebKit du service — chaque service a son propre
/// data_directory, donc son propre processus navigateur + GPU + utilitaires.
fn close_service_webview(app: &AppHandle, id: &str, keep_badge: bool) -> Result<(), TauriumError> {
    let window = app.get_window("main").ok_or(TauriumError::WindowNotFound)?;
    let app_handle = app.clone();
    let id_owned = id.to_string();
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), TauriumError>>();

    window.run_on_main_thread(move || {
        let state = app_handle.state::<WebviewState>();
        let result = close_service_webview_inner(&app_handle, &state, &id_owned, keep_badge);
        let _ = tx.send(result);
    })?;

    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(result) => result,
        Err(_) => Err(TauriumError::ServiceNotFound(format!(
            "Timed out closing webview '{}' on main thread",
            id
        ))),
    }
}

/// Supprime une webview de service et tout son état (badge compris).
pub fn remove_service_webview(app: &AppHandle, id: &str) -> Result<(), TauriumError> {
    close_service_webview(app, id, false)
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

    // Drop badges for services just switched to "off" so the sidebar reload
    // below reflects it, then sync the taskbar unread indicator.
    let total = refresh_badges_for_levels(state);
    update_taskbar_indicator(app, total);

    // Refresh sidebar
    if let Some(sidebar) = app.get_webview("sidebar") {
        sidebar
            .eval("window.__reloadSidebar && window.__reloadSidebar()")
            .ok();
    }

    Ok(())
}

/// Hibernate inactive webviews to save memory.
///
/// Hibernation CLOSES the webview instead of navigating it to about:blank:
/// with one data_directory per service, even a blank webview keeps a full
/// standalone WebView2/WebKit process tree alive (~60-80 MB). Closing frees
/// all of it; switch_to() recreates the webview on the next click (the reload
/// cost is the same as the old about:blank approach). The unread badge is
/// kept so the sidebar still shows pending notifications.
///
/// The idle delay comes from the `hibernation_minutes` preference
/// (default 10); `0` disables hibernation entirely.
pub fn check_hibernation(app: &AppHandle, state: &WebviewState) {
    let hibernation_minutes = load_preferences(&state.app_data_dir).hibernation_minutes;
    if hibernation_minutes == 0 {
        return;
    }
    let hibernation_secs = u64::from(hibernation_minutes) * 60;

    let active = match state.active_id.lock() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            return;
        }
    };
    let now = Instant::now();

    // Services marked "keep alive" are exempt from hibernation: unloading them
    // would stop their background JS, so they'd stop emitting title changes
    // and Taurium would stop detecting new messages until the user manually
    // switches back to them.
    let keep_alive_ids: HashSet<String> = match state.services.lock() {
        Ok(services) => services
            .iter()
            .filter(|s| s.keep_alive)
            .map(|s| s.id.clone())
            .collect(),
        Err(e) => {
            eprintln!("[Taurium] Mutex poisoned: {}", e);
            HashSet::new()
        }
    };

    // Collect candidates, then RELEASE the locks before closing: closing runs
    // on the main thread and re-locks this state, so holding the guards here
    // would deadlock (until the 5s timeout) on every hibernation.
    let to_hibernate = {
        let last_activity = match state.last_activity.lock() {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("[Taurium] Mutex poisoned: {}", e);
                return;
            }
        };
        let navigated = match state.navigated.lock() {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("[Taurium] Mutex poisoned: {}", e);
                return;
            }
        };
        let navigated_ids = filter_hibernation_candidates(&navigated, &keep_alive_ids);
        select_webviews_to_hibernate(
            &navigated_ids,
            active.as_deref(),
            &last_activity,
            now,
            hibernation_secs,
        )
    };

    for id in to_hibernate {
        eprintln!(
            "[Taurium] Hibernating webview: {} (closing process tree)",
            id
        );
        if let Err(e) = close_service_webview(app, &id, true) {
            eprintln!("[Taurium] Failed to hibernate '{}': {}", id, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::time::{Duration, Instant};

    use super::{
        classify_popup_url, cleanup_service_webview_state, compute_service_changes,
        filter_hibernation_candidates, is_meaningful_page_url, notification_body_for_badge_change,
        select_webviews_to_hibernate, service_user_agent_changed, window_location_replace_js,
        PopupTarget, WebviewState,
    };
    use crate::config::{Service, ServicesLoadInfo};
    use tauri::Url;

    fn state_with_service(id: &str) -> WebviewState {
        let now = Instant::now();
        WebviewState {
            created_ids: std::sync::Mutex::new(vec![id.to_string()]),
            active_id: std::sync::Mutex::new(None),
            app_data_dir: std::path::PathBuf::new(),
            services: std::sync::Mutex::new(Vec::new()),
            navigated: std::sync::Mutex::new(HashSet::from([id.to_string()])),
            last_activity: std::sync::Mutex::new(HashMap::from([(id.to_string(), now)])),
            badge_counts: std::sync::Mutex::new(HashMap::from([(id.to_string(), 3u32)])),
            sidebar_width: std::sync::Mutex::new(super::SIDEBAR_WIDTH),
            services_load_info: ServicesLoadInfo {
                filtered_url_count: 0,
                load_error: None,
            },
        }
    }

    #[test]
    fn cleanup_keeps_badge_on_hibernation() {
        let state = state_with_service("svc");
        cleanup_service_webview_state(&state, "svc", true).unwrap();
        assert!(state.created_ids.lock().unwrap().is_empty());
        assert!(state.navigated.lock().unwrap().is_empty());
        assert!(state.last_activity.lock().unwrap().is_empty());
        // The unread badge must survive hibernation so the sidebar keeps
        // showing pending notifications for the closed webview.
        assert_eq!(state.badge_counts.lock().unwrap().get("svc"), Some(&3));
    }

    #[test]
    fn cleanup_drops_badge_on_removal() {
        let state = state_with_service("svc");
        cleanup_service_webview_state(&state, "svc", false).unwrap();
        assert!(state.badge_counts.lock().unwrap().is_empty());
    }

    #[test]
    fn test_is_meaningful_page_url() {
        assert!(!is_meaningful_page_url(""));
        assert!(!is_meaningful_page_url("about:blank"));
        assert!(is_meaningful_page_url("https://example.com"));
    }

    fn popup_url(u: &str) -> Url {
        u.parse().unwrap()
    }

    #[test]
    fn popup_microsoft_login_stays_in_view() {
        assert_eq!(
            classify_popup_url(
                &popup_url("https://login.microsoftonline.com/common/oauth2/v2.0/authorize"),
                "teams.microsoft.com"
            ),
            PopupTarget::SameView
        );
    }

    #[test]
    fn popup_personal_teams_stays_in_view() {
        assert_eq!(
            classify_popup_url(
                &popup_url("https://teams.live.com/v2/"),
                "teams.microsoft.com"
            ),
            PopupTarget::SameView
        );
    }

    #[test]
    fn popup_same_site_stays_in_view() {
        assert_eq!(
            classify_popup_url(
                &popup_url("https://outlook.office365.example.com/pop-out"),
                "mail.example.com"
            ),
            PopupTarget::SameView
        );
        assert_eq!(
            classify_popup_url(
                &popup_url("https://teams.microsoft.com/v2/meeting-popout"),
                "teams.microsoft.com"
            ),
            PopupTarget::SameView
        );
    }

    #[test]
    fn popup_about_blank_opens_popup_window() {
        assert_eq!(
            classify_popup_url(&popup_url("about:blank"), "teams.microsoft.com"),
            PopupTarget::PopupWindow
        );
    }

    #[test]
    fn popup_idp_suffix_stays_in_view() {
        assert_eq!(
            classify_popup_url(&popup_url("https://acme.okta.com/login"), "app.slack.com"),
            PopupTarget::SameView
        );
    }

    #[test]
    fn popup_external_link_opens_system_browser() {
        assert_eq!(
            classify_popup_url(
                &popup_url("https://en.wikipedia.org/wiki/Rust"),
                "teams.microsoft.com"
            ),
            PopupTarget::SystemBrowser
        );
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
            notify: None,
            keep_alive: false,
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
    fn filter_hibernation_candidates_excludes_keep_alive() {
        let navigated: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let keep_alive: HashSet<String> = ["b"].iter().map(|s| s.to_string()).collect();
        let mut candidates = filter_hibernation_candidates(&navigated, &keep_alive);
        candidates.sort();
        assert_eq!(candidates, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn filter_hibernation_candidates_empty_keep_alive_is_noop() {
        let navigated: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let mut candidates = filter_hibernation_candidates(&navigated, &HashSet::new());
        candidates.sort();
        assert_eq!(candidates, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn filter_hibernation_candidates_all_keep_alive_is_empty() {
        let navigated: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let candidates = filter_hibernation_candidates(&navigated, &navigated);
        assert!(candidates.is_empty());
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
            notify: None,
            keep_alive: false,
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
