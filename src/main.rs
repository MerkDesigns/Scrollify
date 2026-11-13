#![cfg_attr(not(test), windows_subsystem = "windows")]
#![cfg(windows)]

//! Scrollify: hold a modifier and scroll to change an app's volume.
//!
//! Anti-cheat friendly by design: no input hooks, no simulated input, no
//! code or memory access in other processes, no process handles, no drivers,
//! no admin rights.

mod app;
mod audio;
mod autostart;
mod config;
mod input;
mod single_instance;
mod tray;
mod ui;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return;
        }
    }

    if let Some(_lock) = single_instance::InstanceLock::acquire() {
        autostart::refresh_shortcuts();
        let _ = app::run();
    }

    unsafe { CoUninitialize() };
}
