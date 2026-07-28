use std::sync::{mpsc::Sender, Arc, Mutex, OnceLock};

use crate::audio::{AudioCommand, WheelDirection};
use crate::config::{normalize_volume_step, ConfigStore, Settings};
use crate::shortcuts;

static APP_STATE: OnceLock<Arc<AppState>> = OnceLock::new();

pub struct AppState {
    settings: Mutex<Settings>,
    store: ConfigStore,
    audio_tx: Sender<AudioCommand>,
}

impl AppState {
    pub fn new(settings: Settings, store: ConfigStore, audio_tx: Sender<AudioCommand>) -> Self {
        Self {
            settings: Mutex::new(settings),
            store,
            audio_tx,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().expect("settings poisoned").clone()
    }

    pub fn adjust_target_volume(&self, direction: WheelDirection) {
        let settings = self.settings();
        if !settings.enabled {
            return;
        }

        let _ = self.audio_tx.send(AudioCommand::Adjust {
            step_db: settings.volume_step_db,
            direction,
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.update_settings(|settings| settings.enabled = enabled);
    }

    pub fn set_volume_step_db(&self, step: f32) {
        self.update_settings(|settings| settings.volume_step_db = normalize_volume_step(step));
    }

    pub fn set_start_with_windows(&self, enabled: bool) {
        shortcuts::set_start_with_windows(enabled);
    }

    pub fn start_with_windows(&self) -> bool {
        shortcuts::is_start_with_windows()
    }

    pub fn shutdown_audio(&self) {
        let _ = self.audio_tx.send(AudioCommand::Shutdown);
    }

    fn update_settings(&self, update: impl FnOnce(&mut Settings)) {
        let snapshot = {
            let mut settings = self.settings.lock().expect("settings poisoned");
            update(&mut settings);
            settings.clone()
        };
        self.store.save(&snapshot);
    }
}

pub fn set_global(state: Arc<AppState>) {
    let _ = APP_STATE.set(state);
}

pub fn global() -> Option<Arc<AppState>> {
    APP_STATE.get().cloned()
}
