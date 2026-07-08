use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: String,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// Zoom CSS factor; absent/`None` is treated as 1.0 everywhere.
    #[serde(default)]
    pub zoom: Option<f64>,
    /// Optional group label used to cluster services in the sidebar.
    /// Absent/`None`/empty means the service is ungrouped.
    #[serde(default)]
    pub group: Option<String>,
    /// Per-service notification level: `"all"` (desktop notification + badge),
    /// `"badge"` (silent unread badge only) or `"off"` (muted, excluded from
    /// badges and the taskbar count). Absent/unknown is treated as `"all"`.
    #[serde(default)]
    pub notify: Option<String>,
}

/// Notification levels (see [`Service::notify`]).
pub const NOTIFY_ALL: &str = "all";
pub const NOTIFY_BADGE: &str = "badge";
pub const NOTIFY_OFF: &str = "off";

impl Service {
    /// Normalized notification level; absent/unknown values fall back to `"all"`.
    pub fn notify_level(&self) -> &'static str {
        match self.notify.as_deref() {
            Some(NOTIFY_BADGE) => NOTIFY_BADGE,
            Some(NOTIFY_OFF) => NOTIFY_OFF,
            _ => NOTIFY_ALL,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_icon_size")]
    pub icon_size: u32,
    /// Legacy field kept for file compatibility; the V3 Snow theme no longer
    /// reads it (surfaces come from the design-system tokens).
    #[serde(default = "default_sidebar_color")]
    pub sidebar_color: String,
    /// V3 Snow accent preset name ("blue", "emerald", "violet", "gold",
    /// "raspberry", "lagoon") — never a free-form color.
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    /// "dark" (default), "light", or "auto" (follows the system scheme).
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
    /// Whether the sidebar is pinned expanded (labels + group names visible).
    #[serde(default = "default_sidebar_expanded")]
    pub sidebar_expanded: bool,
}

fn default_icon_size() -> u32 {
    40
}
fn default_sidebar_color() -> String {
    "#1a1918".to_string()
}
fn default_accent_color() -> String {
    "blue".to_string()
}
fn default_theme() -> String {
    "dark".to_string()
}

const ACCENT_PRESETS: [&str; 6] = ["blue", "emerald", "violet", "gold", "raspberry", "lagoon"];
const THEMES: [&str; 3] = ["auto", "dark", "light"];
fn default_notifications_enabled() -> bool {
    true
}
fn default_sidebar_expanded() -> bool {
    false
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            icon_size: default_icon_size(),
            sidebar_color: default_sidebar_color(),
            accent_color: default_accent_color(),
            theme: default_theme(),
            notifications_enabled: default_notifications_enabled(),
            sidebar_expanded: default_sidebar_expanded(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "{e}"),
            ConfigError::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(e) => Some(e),
            ConfigError::Json(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        ConfigError::Io(value)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        ConfigError::Json(value)
    }
}

#[derive(Debug)]
pub enum LoadServicesError {
    CorruptedJson {
        backup_path: PathBuf,
        parse_error: serde_json::Error,
    },
    Io(std::io::Error),
}

impl fmt::Display for LoadServicesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadServicesError::CorruptedJson {
                backup_path,
                parse_error,
            } => write!(
                f,
                "services.json is corrupted (backed up to {}): {}",
                backup_path.display(),
                parse_error
            ),
            LoadServicesError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadServicesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadServicesError::CorruptedJson { parse_error, .. } => Some(parse_error),
            LoadServicesError::Io(e) => Some(e),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ServicesLoadInfo {
    pub filtered_url_count: usize,
    pub load_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoadServicesResult {
    pub services: Vec<Service>,
    /// `true` when `services.json` was missing and default services were created.
    pub created_defaults: bool,
    /// Number of entries removed because the URL was invalid or not http(s).
    pub filtered_url_count: usize,
}

pub fn load_preferences(app_data_dir: &Path) -> Preferences {
    let path = app_data_dir.join("preferences.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut prefs: Preferences = serde_json::from_str(&content).unwrap_or_default();
    // Migration : les anciens accents étaient des couleurs hexadécimales
    // libres ; tout ce qui n'est pas un preset V3 Snow retombe sur "blue".
    if !ACCENT_PRESETS.contains(&prefs.accent_color.as_str()) {
        prefs.accent_color = default_accent_color();
    }
    if !THEMES.contains(&prefs.theme.as_str()) {
        prefs.theme = default_theme();
    }
    prefs
}

pub fn save_preferences(app_data_dir: &Path, prefs: &Preferences) -> Result<(), ConfigError> {
    let path = app_data_dir.join("preferences.json");
    fs::create_dir_all(app_data_dir)?;
    let json = serde_json::to_string_pretty(prefs)?;
    fs::write(&path, json)?;
    Ok(())
}

pub fn get_services_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("services.json")
}

/// Only `http`/`https` service URLs are accepted; everything else
/// (`javascript:`, `file:`, `ftp:`, garbage…) is rejected.
fn is_valid_service_url(raw: &str) -> bool {
    matches!(
        raw.parse::<url::Url>(),
        Ok(u) if matches!(u.scheme(), "http" | "https")
    )
}

/// Services créés au premier lancement (fichier absent).
fn default_services() -> Vec<Service> {
    vec![
        Service {
            id: "default-whatsapp".to_string(),
            name: "WhatsApp Web".to_string(),
            url: "https://web.whatsapp.com".to_string(),
            icon: "lucide:MessageCircle".to_string(),
            user_agent: None,
            zoom: None,
            group: Some("Personnel".to_string()),
            notify: None,
        },
        Service {
            id: "default-gmail".to_string(),
            name: "Gmail".to_string(),
            url: "https://mail.google.com".to_string(),
            icon: "lucide:Mail".to_string(),
            user_agent: None,
            zoom: None,
            group: Some("Personnel".to_string()),
            notify: None,
        },
        Service {
            id: "default-discord".to_string(),
            name: "Discord".to_string(),
            url: "https://discord.com/app".to_string(),
            icon: "lucide:Gamepad2".to_string(),
            user_agent: None,
            zoom: None,
            group: Some("Personnel".to_string()),
            notify: None,
        },
        Service {
            id: "default-slack".to_string(),
            name: "Slack".to_string(),
            url: "https://app.slack.com".to_string(),
            icon: "lucide:Hash".to_string(),
            user_agent: None,
            zoom: None,
            group: Some("Travail".to_string()),
            notify: None,
        },
    ]
}

fn services_backup_path(path: &Path) -> PathBuf {
    path.with_file_name("services.json.bak")
}

fn backup_corrupted_services_file(path: &Path) -> Result<PathBuf, LoadServicesError> {
    let backup_path = services_backup_path(path);
    if backup_path.exists() {
        fs::remove_file(&backup_path).map_err(LoadServicesError::Io)?;
    }
    fs::rename(path, &backup_path).map_err(LoadServicesError::Io)?;
    Ok(backup_path)
}

fn normalize_service(mut service: Service) -> Service {
    if service.user_agent.as_deref() == Some("") {
        service.user_agent = None;
    }
    if let Some(z) = service.zoom {
        if !z.is_finite() || (z - 1.0).abs() < f64::EPSILON {
            service.zoom = None;
        }
    }
    if let Some(g) = service.group.as_deref() {
        if g.trim().is_empty() {
            service.group = None;
        }
    }
    // Drop unknown/empty/default notify levels so the file stays clean and
    // `notify_level()` doesn't have to guess (absent == "all").
    match service.notify.as_deref() {
        Some(NOTIFY_BADGE) | Some(NOTIFY_OFF) => {}
        _ => service.notify = None,
    }
    service
}

fn sanitize_services(raw_services: Vec<Service>) -> (Vec<Service>, usize) {
    let total = raw_services.len();
    let services: Vec<Service> = raw_services
        .into_iter()
        .filter(|service| is_valid_service_url(&service.url))
        .map(normalize_service)
        .collect();
    let filtered_url_count = total.saturating_sub(services.len());
    (services, filtered_url_count)
}

pub fn load_services(app_data_dir: &Path) -> Result<LoadServicesResult, LoadServicesError> {
    let path = get_services_path(app_data_dir);

    if !path.exists() {
        fs::create_dir_all(app_data_dir).map_err(LoadServicesError::Io)?;
        let defaults = default_services();
        let json = serde_json::to_string_pretty(&defaults).map_err(|err| {
            LoadServicesError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            ))
        })?;
        fs::write(&path, json).map_err(LoadServicesError::Io)?;
        return Ok(LoadServicesResult {
            services: defaults,
            created_defaults: true,
            filtered_url_count: 0,
        });
    }

    let content = fs::read_to_string(&path).map_err(LoadServicesError::Io)?;
    let raw_services: Vec<Service> = match serde_json::from_str(&content) {
        Ok(services) => services,
        Err(parse_error) => {
            let backup_path = backup_corrupted_services_file(&path)?;
            return Err(LoadServicesError::CorruptedJson {
                backup_path,
                parse_error,
            });
        }
    };

    let (services, filtered_url_count) = sanitize_services(raw_services);

    Ok(LoadServicesResult {
        services,
        created_defaults: false,
        filtered_url_count,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    pub last_active_service: Option<String>,
}

pub fn load_state(app_data_dir: &Path) -> AppState {
    let path = app_data_dir.join("state.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_services(app_data_dir: &Path, services: &[Service]) -> Result<(), ConfigError> {
    let path = get_services_path(app_data_dir);
    fs::create_dir_all(app_data_dir)?;
    let json = serde_json::to_string_pretty(services)?;
    fs::write(&path, json)?;
    Ok(())
}

pub fn save_state(app_data_dir: &Path, state: &AppState) -> Result<(), ConfigError> {
    let path = app_data_dir.join("state.json");
    fs::create_dir_all(app_data_dir)?;
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&path, json)?;
    Ok(())
}

/// Upper bound for badge counts extracted from page titles (years, IDs, etc. are ignored).
const MAX_BADGE_COUNT: u32 = 999;

pub fn extract_badge_count(title: &str) -> u32 {
    // Match patterns like "(3)", "(12)", "[5]" in page titles
    static RE_PAREN: OnceLock<regex_lite::Regex> = OnceLock::new();
    static RE_BRACKET: OnceLock<regex_lite::Regex> = OnceLock::new();

    let re_paren = RE_PAREN.get_or_init(|| {
        regex_lite::Regex::new(r"\((\d+)\)").expect("valid badge regex pattern for parentheses")
    });
    let re_bracket = RE_BRACKET.get_or_init(|| {
        regex_lite::Regex::new(r"\[(\d+)\]").expect("valid badge regex pattern for brackets")
    });

    if let Some(caps) = re_paren.captures(title) {
        if let Ok(n) = caps[1].parse::<u32>() {
            if n > 0 && n <= MAX_BADGE_COUNT {
                return n;
            }
        }
    }
    if let Some(caps) = re_bracket.captures(title) {
        if let Ok(n) = caps[1].parse::<u32>() {
            if n > 0 && n <= MAX_BADGE_COUNT {
                return n;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_extract_badge_count() {
        assert_eq!(extract_badge_count("(3) Slack"), 3);
        assert_eq!(extract_badge_count("[12] Discord"), 12);
        assert_eq!(extract_badge_count("Gmail - Inbox"), 0);
        assert_eq!(extract_badge_count("(0) Nothing"), 0);
        assert_eq!(extract_badge_count("((5)) nested"), 5);
        assert_eq!(extract_badge_count(""), 0);
        assert_eq!(extract_badge_count("about:blank"), 0);
        assert_eq!(extract_badge_count("(999) many"), 999);
        assert_eq!(extract_badge_count("(2025) Rapport"), 0);
        assert_eq!(extract_badge_count("(1500)"), 0);
        assert_eq!(extract_badge_count("(99)"), 99);
    }

    fn service_with_notify(notify: Option<&str>) -> Service {
        Service {
            id: "svc".to_string(),
            name: "Svc".to_string(),
            url: "https://example.com".to_string(),
            icon: "x".to_string(),
            user_agent: None,
            zoom: None,
            group: None,
            notify: notify.map(str::to_string),
        }
    }

    #[test]
    fn test_notify_level_defaults_to_all() {
        assert_eq!(service_with_notify(None).notify_level(), NOTIFY_ALL);
        assert_eq!(service_with_notify(Some("all")).notify_level(), NOTIFY_ALL);
        assert_eq!(
            service_with_notify(Some("badge")).notify_level(),
            NOTIFY_BADGE
        );
        assert_eq!(service_with_notify(Some("off")).notify_level(), NOTIFY_OFF);
        // Unknown/garbage falls back to "all".
        assert_eq!(
            service_with_notify(Some("bogus")).notify_level(),
            NOTIFY_ALL
        );
        assert_eq!(service_with_notify(Some("")).notify_level(), NOTIFY_ALL);
    }

    #[test]
    fn test_normalize_service_notify() {
        // "all"/unknown/empty are dropped to None; "badge"/"off" are kept.
        assert_eq!(
            normalize_service(service_with_notify(Some("all"))).notify,
            None
        );
        assert_eq!(
            normalize_service(service_with_notify(Some("bogus"))).notify,
            None
        );
        assert_eq!(
            normalize_service(service_with_notify(Some(""))).notify,
            None
        );
        assert_eq!(normalize_service(service_with_notify(None)).notify, None);
        assert_eq!(
            normalize_service(service_with_notify(Some("badge"))).notify,
            Some("badge".to_string())
        );
        assert_eq!(
            normalize_service(service_with_notify(Some("off"))).notify,
            Some("off".to_string())
        );
    }

    #[test]
    fn test_load_services() {
        let dir = tempdir().expect("tempdir should be created");
        let app_data_dir = dir.path().to_path_buf();
        let services_path = get_services_path(&app_data_dir);

        // Missing file: should be created with built-in defaults and load successfully.
        assert!(!services_path.exists());
        let loaded_missing =
            load_services(&app_data_dir).expect("missing file should load defaults");
        assert!(services_path.exists());
        assert!(loaded_missing.created_defaults);
        assert_eq!(loaded_missing.filtered_url_count, 0);
        assert_eq!(loaded_missing.services.len(), 4);
        let ids: Vec<&str> = loaded_missing
            .services
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "default-whatsapp",
                "default-gmail",
                "default-discord",
                "default-slack"
            ]
        );

        // Valid file: should load entries and filter invalid / non-http(s) URLs.
        let valid_json = r#"[
            {"id":"ok","name":"Ok","url":"https://example.com","icon":"ok.svg"},
            {"id":"bad-url","name":"Bad","url":"not-a-url","icon":"bad.svg"},
            {"id":"ftp","name":"FTP","url":"ftp://example.com","icon":"ftp.svg"},
            {"id":"js-scheme","name":"Js","url":"javascript:alert(1)","icon":"x"},
            {"id":"file-scheme","name":"File","url":"file:///etc/passwd","icon":"x"}
        ]"#;
        fs::write(&services_path, valid_json).expect("valid services.json should be written");
        let loaded_valid = load_services(&app_data_dir).expect("valid services.json should load");
        assert!(!loaded_valid.created_defaults);
        assert_eq!(loaded_valid.filtered_url_count, 4);
        assert_eq!(loaded_valid.services.len(), 1);
        assert_eq!(loaded_valid.services[0].id, "ok");

        // Invalid JSON: should error, back up the file, and never overwrite it.
        fs::write(&services_path, "{ invalid json")
            .expect("invalid services.json should be written");
        let backup_path = services_backup_path(&services_path);
        let loaded_invalid = load_services(&app_data_dir);
        assert!(loaded_invalid.is_err());
        assert!(!services_path.exists());
        assert!(backup_path.exists());
        let backup_content = fs::read_to_string(&backup_path).expect("backup should be readable");
        assert_eq!(backup_content, "{ invalid json");

        match loaded_invalid {
            Err(LoadServicesError::CorruptedJson {
                backup_path: reported_backup,
                ..
            }) => assert_eq!(reported_backup, backup_path),
            other => panic!("expected corrupted JSON error, got {other:?}"),
        }
    }

    #[test]
    fn test_save_services_propagates_io_error() {
        let dir = tempdir().expect("tempdir should be created");
        let app_data_dir = dir.path().join("blocked");
        fs::write(&app_data_dir, "not a directory").expect("blocking file should be written");

        let services = default_services();
        let result = save_services(&app_data_dir, &services);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_preferences() {
        let dir = tempdir().expect("tempdir should be created");
        let app_data_dir = dir.path().to_path_buf();
        let prefs_path = app_data_dir.join("preferences.json");

        // Empty file content: invalid JSON => default preferences.
        fs::create_dir_all(&app_data_dir).expect("app data dir should be created");
        fs::write(&prefs_path, "").expect("empty preferences.json should be written");
        let empty = load_preferences(&app_data_dir);
        assert_eq!(empty.icon_size, 40);
        assert_eq!(empty.accent_color, "blue");
        assert_eq!(empty.theme, "dark");
        assert!(empty.notifications_enabled);

        // Partial file: only icon_size provided, rest should use defaults.
        fs::write(&prefs_path, r#"{"icon_size":72}"#)
            .expect("partial preferences.json should be written");
        let partial = load_preferences(&app_data_dir);
        assert_eq!(partial.icon_size, 72);
        assert_eq!(partial.accent_color, "blue");
        assert_eq!(partial.theme, "dark");
        assert!(partial.notifications_enabled);

        // Legacy free-form accent colors and unknown themes fall back to the
        // V3 Snow defaults (calibrated presets only).
        let json = serde_json::to_string(&serde_json::json!({
            "accent_color": "#e94560",
            "theme": "sepia"
        }))
        .expect("preferences JSON should serialize");
        fs::write(&prefs_path, json).expect("legacy preferences.json should be written");
        let legacy = load_preferences(&app_data_dir);
        assert_eq!(legacy.accent_color, "blue");
        assert_eq!(legacy.theme, "dark");

        // Full file: all values should be loaded.
        let json = serde_json::to_string(&serde_json::json!({
            "icon_size": 24,
            "accent_color": "raspberry",
            "theme": "light",
            "notifications_enabled": false
        }))
        .expect("preferences JSON should serialize");
        fs::write(&prefs_path, json).expect("full preferences.json should be written");
        let full = load_preferences(&app_data_dir);
        assert_eq!(full.icon_size, 24);
        assert_eq!(full.accent_color, "raspberry");
        assert_eq!(full.theme, "light");
        assert!(!full.notifications_enabled);
    }
}
