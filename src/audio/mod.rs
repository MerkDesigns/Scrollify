//! Audio worker thread: finds the target apps' mixer sessions and changes
//! their volume through the Windows Core Audio API (the same API the Windows
//! volume mixer uses).

mod sessions;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::{
    eRender, AudioSessionStateExpired, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub use sessions::display_name;

use crate::input::Direction;

pub enum Command {
    Adjust {
        direction: Direction,
        step_db: f32,
        targets: Vec<String>,
    },
    ListApps,
    Shutdown,
}

pub enum Report {
    /// New volume (0..=1) of the first target app that was changed.
    Volume { app: String, volume: f32 },
    /// Apps that currently have an audio session, sorted by name.
    Apps(Vec<String>),
}

pub struct AudioWorker {
    commands: Sender<Command>,
    reports: Receiver<Report>,
    thread: Option<JoinHandle<()>>,
}

impl AudioWorker {
    /// Starts the worker. After each report it posts `notify_msg` to `notify`.
    pub fn start(notify: HWND, notify_msg: u32) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (report_tx, reports) = mpsc::channel();
        let notify = notify.0 as isize;
        let thread = thread::spawn(move || {
            let send = |report: Report| {
                if report_tx.send(report).is_ok() {
                    unsafe {
                        let hwnd = HWND(notify as *mut _);
                        let _ = PostMessageW(hwnd, notify_msg, WPARAM(0), LPARAM(0));
                    }
                }
            };
            run(command_rx, send);
        });
        Self {
            commands,
            reports,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn try_report(&self) -> Option<Report> {
        self.reports.try_recv().ok()
    }
}

impl Drop for AudioWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(commands: Receiver<Command>, send: impl Fn(Report)) {
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return;
        }
    }
    let enumerator: Option<IMMDeviceEnumerator> =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() };

    if let Some(enumerator) = enumerator {
        while let Ok(command) = commands.recv() {
            match command {
                Command::Adjust {
                    direction,
                    step_db,
                    targets,
                } => {
                    if let Some((app, volume)) = adjust(&enumerator, direction, step_db, &targets) {
                        send(Report::Volume { app, volume });
                    }
                }
                Command::ListApps => send(Report::Apps(list_apps(&enumerator))),
                Command::Shutdown => break,
            }
        }
    }

    unsafe { CoUninitialize() };
}

/// Changes every matching session on every active output device. Returns the
/// first changed app and its new volume.
fn adjust(
    enumerator: &IMMDeviceEnumerator,
    direction: Direction,
    step_db: f32,
    targets: &[String],
) -> Option<(String, f32)> {
    let mut first = None;
    for_each_session(enumerator, |exe, control| {
        if !sessions::is_target(exe, targets) {
            return;
        }
        let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
            return;
        };
        unsafe {
            let Ok(current) = volume.GetMasterVolume() else {
                return;
            };
            let next = sessions::step_volume(current, step_db, direction);
            if volume.SetMasterVolume(next, std::ptr::null()).is_ok() && first.is_none() {
                first = Some((exe.to_string(), next));
            }
        }
    });
    first
}

fn list_apps(enumerator: &IMMDeviceEnumerator) -> Vec<String> {
    let mut apps: Vec<String> = Vec::new();
    for_each_session(enumerator, |exe, _| {
        if !apps.iter().any(|a| a.eq_ignore_ascii_case(exe)) {
            apps.push(exe.to_string());
        }
    });
    apps.sort_by_key(|a| a.to_ascii_lowercase());
    apps
}

/// Calls `visit(exe_name, session)` for each live session that belongs to an
/// app. Errors on one device or session are skipped.
fn for_each_session(
    enumerator: &IMMDeviceEnumerator,
    mut visit: impl FnMut(&str, &IAudioSessionControl),
) {
    unsafe {
        let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
            return;
        };
        let count = devices.GetCount().unwrap_or(0);
        for d in 0..count {
            let Ok(device) = devices.Item(d) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(list) = manager.GetSessionEnumerator() else {
                continue;
            };
            for s in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(s) else {
                    continue;
                };
                if control
                    .GetState()
                    .is_ok_and(|state| state == AudioSessionStateExpired)
                {
                    continue;
                }
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                let Some(id) = take_string(control2.GetSessionInstanceIdentifier()) else {
                    continue;
                };
                if let Some(exe) = sessions::exe_from_instance_id(&id) {
                    visit(exe, &control);
                }
            }
        }
    }
}

/// Copies a COM-allocated string and frees it.
fn take_string(value: windows::core::Result<PWSTR>) -> Option<String> {
    let ptr = value.ok()?;
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let text = ptr.to_string().ok();
        CoTaskMemFree(Some(ptr.0 as *const _));
        text
    }
}
