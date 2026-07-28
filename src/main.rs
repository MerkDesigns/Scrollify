#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg(windows)]

mod app_state;
mod audio;
mod config;
mod input_hook;
mod settings_ui;
mod shortcuts;
mod single_instance;
mod tray;
mod util;

use std::sync::{mpsc, Arc};

use app_state::AppState;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG,
};

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let _com = ComApartment;

    let _instance = match single_instance::SingleInstance::acquire()? {
        Some(instance) => instance,
        None => return Ok(()),
    };

    shortcuts::ensure_start_menu_shortcut();
    run()
}

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn run() -> windows::core::Result<()> {
    let store = config::ConfigStore::new();
    let (audio_tx, audio_rx) = mpsc::channel();
    let audio_thread = audio::spawn_audio_thread(audio_rx);

    let app_state = Arc::new(AppState::new(store.load(), store, audio_tx));
    app_state::set_global(app_state.clone());

    let settings_ui = settings_ui::SettingsUi::start(app_state.clone())?;
    let tray = tray::TrayApp::new(app_state.clone(), settings_ui.handle())?;
    let hook = input_hook::MouseHook::install()?;

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if msg.message == settings_ui::WM_REFRESH_TRAY_ICON {
                tray.refresh_icon();
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    drop(hook);
    drop(tray);
    drop(settings_ui);
    app_state.shutdown_audio();
    let _ = audio_thread.join();
    Ok(())
}
