use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: String,
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

fn default_icon_size() -> u32 { 40 }
fn default_sidebar_color() -> String { "#16213e".to_string() }
fn default_accent_color() -> String { "#e94560".to_string() }
fn default_notifications_enabled() -> bool { true }

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

pub fn load_preferences(app_data_dir: &PathBuf) -> Preferences {
    let path = app_data_dir.join("preferences.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_preferences(app_data_dir: &PathBuf, prefs: &Preferences) {
    let path = app_data_dir.join("preferences.json");
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        fs::write(&path, json).ok();
    }
}

pub fn get_services_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("services.json")
}

pub fn load_services(app_data_dir: &PathBuf) -> Vec<Service> {
    let path = get_services_path(app_data_dir);

    if !path.exists() {
        let default_services = include_str!("../../services.json");
        fs::create_dir_all(app_data_dir).ok();
        fs::write(&path, default_services).ok();
    }

    let content = fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
    let services: Vec<Service> = serde_json::from_str(&content).unwrap_or_default();

    // Filter out services with invalid URLs
    services
        .into_iter()
        .filter(|s| s.url.parse::<url::Url>().is_ok())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    pub last_active_service: Option<String>,
}

pub fn load_state(app_data_dir: &PathBuf) -> AppState {
    let path = app_data_dir.join("state.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_services(app_data_dir: &PathBuf, services: &[Service]) {
    let path = get_services_path(app_data_dir);
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(services) {
        fs::write(&path, json).ok();
    }
}

pub fn save_state(app_data_dir: &PathBuf, state: &AppState) {
    let path = app_data_dir.join("state.json");
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(state) {
        fs::write(&path, json).ok();
    }
}

pub fn extract_badge_count(title: &str) -> u32 {
    // Match patterns like "(3)", "(12)", "[5]" in page titles
    let re_paren = regex_lite::Regex::new(r"\((\d+)\)").ok();
    let re_bracket = regex_lite::Regex::new(r"\[(\d+)\]").ok();

    if let Some(re) = re_paren {
        if let Some(caps) = re.captures(title) {
            if let Ok(n) = caps[1].parse::<u32>() {
                if n > 0 {
                    return n;
                }
            }
        }
    }
    if let Some(re) = re_bracket {
        if let Some(caps) = re.captures(title) {
            if let Ok(n) = caps[1].parse::<u32>() {
                if n > 0 {
                    return n;
                }
            }
        }
    }
    0
}

