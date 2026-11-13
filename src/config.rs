//! User settings and their JSON file in `%APPDATA%\Scrollify\config.json`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const MIN_STEP_DB: f32 = 0.5;
pub const MAX_STEP_DB: f32 = 12.0;
pub const STEP_INCREMENT_DB: f32 = 0.5;
pub const DEFAULT_TARGET: &str = "Spotify.exe";

/// Key or mouse button that must be held while scrolling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    #[default]
    Alt,
    Ctrl,
    Shift,
    Win,
    Mouse4,
    Mouse5,
}

impl Modifier {
    pub const ALL: [Modifier; 6] = [
        Modifier::Alt,
        Modifier::Ctrl,
        Modifier::Shift,
        Modifier::Win,
        Modifier::Mouse4,
        Modifier::Mouse5,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Modifier::Alt => "Alt",
            Modifier::Ctrl => "Ctrl",
            Modifier::Shift => "Shift",
            Modifier::Win => "Win",
            Modifier::Mouse4 => "Mouse 4",
            Modifier::Mouse5 => "Mouse 5",
        }
    }

    /// The next (`forward`) or previous modifier, wrapping around.
    pub fn cycle(self, forward: bool) -> Modifier {
        let len = Self::ALL.len();
        let index = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        let next = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        Self::ALL[next]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub volume_step_db: f32,
    pub modifier: Modifier,
    pub targets: Vec<String>,
    pub show_overlay: bool,
    /// Stop modifier + wheel from also scrolling other apps (uses a hook).
    pub block_scroll: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume_step_db: 1.0,
            modifier: Modifier::Alt,
            targets: vec![DEFAULT_TARGET.to_string()],
            show_overlay: true,
            block_scroll: false,
        }
    }
}

impl Settings {
    /// Brings loaded or edited values back into their valid ranges.
    pub fn normalize(&mut self) {
        self.volume_step_db = normalize_step(self.volume_step_db);
        self.targets = normalize_targets(&self.targets);
    }
}

/// Rounds to the nearest 0.5 dB and clamps to the allowed range.
pub fn normalize_step(step: f32) -> f32 {
    if !step.is_finite() {
        return 1.0;
    }
    let rounded = (step / STEP_INCREMENT_DB).round() * STEP_INCREMENT_DB;
    rounded.clamp(MIN_STEP_DB, MAX_STEP_DB)
}

/// Trims names, adds a missing `.exe`, and drops empty or duplicate entries
/// (case-insensitive, first spelling wins).
pub fn normalize_targets(targets: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in targets {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let name = if name.to_ascii_lowercase().ends_with(".exe") {
            name.to_string()
        } else {
            format!("{name}.exe")
        };
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&name))
        {
            out.push(name);
        }
    }
    out
}

pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Self {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            path: base.join("Scrollify").join("config.json"),
        }
    }

    pub fn load(&self) -> Settings {
        let mut settings = fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| parse(&text))
            .unwrap_or_default();
        settings.normalize();
        settings
    }

    pub fn save(&self, settings: &Settings) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            // Write to a temp file first so a crash never leaves a half-written config.
            let tmp = self.path.with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }
}

fn parse(text: &str) -> Option<Settings> {
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_is_rounded_and_clamped() {
        assert_eq!(normalize_step(1.2), 1.0);
        assert_eq!(normalize_step(1.3), 1.5);
        assert_eq!(normalize_step(0.0), 0.5);
        assert_eq!(normalize_step(40.0), 12.0);
        assert_eq!(normalize_step(f32::NAN), 1.0);
    }

    #[test]
    fn old_config_loads_with_new_defaults() {
        let old = r#"{ "enabled": false, "volume_step_db": 2.5 }"#;
        let mut settings = parse(old).unwrap();
        settings.normalize();
        assert!(!settings.enabled);
        assert_eq!(settings.volume_step_db, 2.5);
        assert_eq!(settings.modifier, Modifier::Alt);
        assert_eq!(settings.targets, vec!["Spotify.exe".to_string()]);
        assert!(settings.show_overlay);
        assert!(!settings.block_scroll);
    }

    #[test]
    fn targets_are_cleaned() {
        let input = vec![
            " spotify ".to_string(),
            "Spotify.EXE".to_string(),
            "".to_string(),
            "firefox.exe".to_string(),
        ];
        assert_eq!(
            normalize_targets(&input),
            vec!["spotify.exe".to_string(), "firefox.exe".to_string()]
        );
    }

    #[test]
    fn modifier_round_trips_and_cycles() {
        let json = serde_json::to_string(&Modifier::Mouse4).unwrap();
        assert_eq!(json, "\"mouse4\"");
        assert_eq!(Modifier::Alt.cycle(false), Modifier::Mouse5);
        assert_eq!(Modifier::Mouse5.cycle(true), Modifier::Alt);
    }
}
