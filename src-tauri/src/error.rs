use serde::ser::SerializeStruct;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TauriumError {
    #[error("Webview not found: {0}")]
    WebviewNotFound(String),
    #[error("Service not found: {0}")]
    ServiceNotFound(String),
    #[error("Window not found")]
    WindowNotFound,
    #[error("Mutex poisoned: {0}")]
    MutexPoisoned(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    LoadServices(#[from] crate::config::LoadServicesError),
}

impl Serialize for TauriumError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("TauriumError", 2)?;
        state.serialize_field(
            "type",
            match self {
                TauriumError::WebviewNotFound(_) => "WebviewNotFound",
                TauriumError::ServiceNotFound(_) => "ServiceNotFound",
                TauriumError::WindowNotFound => "WindowNotFound",
                TauriumError::MutexPoisoned(_) => "MutexPoisoned",
                TauriumError::Io(_) => "Io",
                TauriumError::Tauri(_) => "Tauri",
                TauriumError::Serialization(_) => "Serialization",
                TauriumError::Config(_) => "Config",
                TauriumError::LoadServices(_) => "LoadServices",
            },
        )?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}
