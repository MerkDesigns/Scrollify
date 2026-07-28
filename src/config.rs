use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub struct Settings {
    pub enabled: bool,
    pub volume_step_db: f32,
}

pub fn normalize_volume_step(step: f32) -> f32 {
    ((step * 2.0).round() / 2.0).clamp(0.5, 12.0)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume_step_db: 1.0,
        }
    }
}

pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Self {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self {
            path: base.join("Scrollify").join("config.json"),
        }
    }

    pub fn load(&self) -> Settings {
        let Ok(contents) = fs::read_to_string(&self.path) else {
            return Settings::default();
        };

        let mut settings: Settings = serde_json::from_str(&contents).unwrap_or_default();
        settings.volume_step_db = normalize_volume_step(settings.volume_step_db);
        settings
    }

    pub fn save(&self, settings: &Settings) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = fs::write(&self.path, json);
        }
    }
}
