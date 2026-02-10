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

pub fn get_services_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("services.json")
}

pub fn load_services(app_data_dir: &PathBuf) -> Vec<Service> {
    let path = get_services_path(app_data_dir);

    // If services.json doesn't exist in app data dir, copy the default one
    if !path.exists() {
        let default_services = include_str!("../../services.json");
        fs::create_dir_all(app_data_dir).ok();
        fs::write(&path, default_services).ok();
    }

    let content = fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
    serde_json::from_str(&content).unwrap_or_default()
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

pub fn save_state(app_data_dir: &PathBuf, state: &AppState) {
    let path = app_data_dir.join("state.json");
    fs::create_dir_all(app_data_dir).ok();
    if let Ok(json) = serde_json::to_string_pretty(state) {
        fs::write(&path, json).ok();
    }
}
