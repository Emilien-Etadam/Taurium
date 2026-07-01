use serde::{Deserialize, Serialize};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_icon_size")]
    pub icon_size: u32,
    #[serde(default = "default_sidebar_color")]
    pub sidebar_color: String,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
}

fn default_icon_size() -> u32 {
    40
}
fn default_sidebar_color() -> String {
    "#16213e".to_string()
}
fn default_accent_color() -> String {
    "#e94560".to_string()
}
fn default_notifications_enabled() -> bool {
    true
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            icon_size: default_icon_size(),
            sidebar_color: default_sidebar_color(),
            accent_color: default_accent_color(),
            notifications_enabled: default_notifications_enabled(),
        }
    }
}

pub fn load_preferences(app_data_dir: &Path) -> Preferences {
    let path = app_data_dir.join("preferences.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_preferences(app_data_dir: &Path, prefs: &Preferences) {
    let path = app_data_dir.join("preferences.json");
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        fs::write(&path, json).ok();
    }
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
            icon: "💬".to_string(),
            user_agent: None,
            zoom: None,
        },
        Service {
            id: "default-gmail".to_string(),
            name: "Gmail".to_string(),
            url: "https://mail.google.com".to_string(),
            icon: "📧".to_string(),
            user_agent: None,
            zoom: None,
        },
        Service {
            id: "default-discord".to_string(),
            name: "Discord".to_string(),
            url: "https://discord.com/app".to_string(),
            icon: "🎮".to_string(),
            user_agent: None,
            zoom: None,
        },
        Service {
            id: "default-slack".to_string(),
            name: "Slack".to_string(),
            url: "https://app.slack.com".to_string(),
            icon: "💼".to_string(),
            user_agent: None,
            zoom: None,
        },
    ]
}

pub fn load_services(app_data_dir: &Path) -> Vec<Service> {
    let path = get_services_path(app_data_dir);

    if !path.exists() {
        fs::create_dir_all(app_data_dir).ok();
        if let Ok(json) = serde_json::to_string_pretty(&default_services()) {
            fs::write(&path, json).ok();
        }
    }

    let content = fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
    let services: Vec<Service> = serde_json::from_str(&content).unwrap_or_default();

    // Filter out services whose URL is not a valid http/https URL
    services
        .into_iter()
        .filter(|s| is_valid_service_url(&s.url))
        .map(|mut s| {
            if s.user_agent.as_deref() == Some("") {
                s.user_agent = None;
            }
            if let Some(z) = s.zoom {
                if !z.is_finite() || (z - 1.0).abs() < f64::EPSILON {
                    s.zoom = None;
                }
            }
            s
        })
        .collect()
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

pub fn save_services(app_data_dir: &Path, services: &[Service]) {
    let path = get_services_path(app_data_dir);
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(services) {
        fs::write(&path, json).ok();
    }
}

pub fn save_state(app_data_dir: &Path, state: &AppState) {
    let path = app_data_dir.join("state.json");
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(state) {
        fs::write(&path, json).ok();
    }
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

    #[test]
    fn test_load_services() {
        let dir = tempdir().expect("tempdir should be created");
        let app_data_dir = dir.path().to_path_buf();
        let services_path = get_services_path(&app_data_dir);

        // Missing file: should be created with built-in defaults and load successfully.
        assert!(!services_path.exists());
        let loaded_missing = load_services(&app_data_dir);
        assert!(services_path.exists());
        assert_eq!(loaded_missing.len(), 4);
        let ids: Vec<&str> = loaded_missing.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "default-whatsapp",
                "default-gmail",
                "default-discord",
                "default-slack"
            ]
        );

        // Valid file: should load entries and filter non-http(s) / invalid URLs.
        let valid_json = r#"[
            {"id":"ok","name":"Ok","url":"https://example.com","icon":"ok.svg"},
            {"id":"bad-url","name":"Bad","url":"not-a-url","icon":"bad.svg"},
            {"id":"js-scheme","name":"Js","url":"javascript:alert(1)","icon":"x"},
            {"id":"file-scheme","name":"File","url":"file:///etc/passwd","icon":"x"}
        ]"#;
        fs::write(&services_path, valid_json).expect("valid services.json should be written");
        let loaded_valid = load_services(&app_data_dir);
        assert_eq!(loaded_valid.len(), 1);
        assert_eq!(loaded_valid[0].id, "ok");

        // Invalid JSON: should fail gracefully to empty list.
        fs::write(&services_path, "{ invalid json")
            .expect("invalid services.json should be written");
        let loaded_invalid = load_services(&app_data_dir);
        assert!(loaded_invalid.is_empty());
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
        assert_eq!(empty.sidebar_color, "#16213e");
        assert_eq!(empty.accent_color, "#e94560");
        assert!(empty.notifications_enabled);

        // Partial file: only icon_size provided, rest should use defaults.
        fs::write(&prefs_path, r#"{"icon_size":72}"#)
            .expect("partial preferences.json should be written");
        let partial = load_preferences(&app_data_dir);
        assert_eq!(partial.icon_size, 72);
        assert_eq!(partial.sidebar_color, "#16213e");
        assert_eq!(partial.accent_color, "#e94560");
        assert!(partial.notifications_enabled);

        // Full file: all values should be loaded.
        let sidebar_color = "#000000";
        let accent_color = "#ffffff";
        let json = serde_json::to_string(&serde_json::json!({
            "icon_size": 24,
            "sidebar_color": sidebar_color,
            "accent_color": accent_color,
            "notifications_enabled": false
        }))
        .expect("preferences JSON should serialize");
        fs::write(&prefs_path, json).expect("full preferences.json should be written");
        let full = load_preferences(&app_data_dir);
        assert_eq!(full.icon_size, 24);
        assert_eq!(full.sidebar_color, "#000000");
        assert_eq!(full.accent_color, "#ffffff");
        assert!(!full.notifications_enabled);
    }
}
