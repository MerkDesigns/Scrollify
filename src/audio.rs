use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::mpsc::Receiver;
use std::thread;

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    eRender, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

const TARGET_PROCESS: &str = "Spotify.exe";

#[derive(Clone, Copy)]
pub enum WheelDirection {
    Up,
    Down,
}

pub enum AudioCommand {
    Adjust {
        step_db: f32,
        direction: WheelDirection,
    },
    Shutdown,
}

pub fn spawn_audio_thread(rx: Receiver<AudioCommand>) -> thread::JoinHandle<()> {
    thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return;
        }

        while let Ok(command) = rx.recv() {
            match command {
                AudioCommand::Adjust { step_db, direction } => {
                    let _ = adjust_process_volume(step_db, direction);
                }
                AudioCommand::Shutdown => break,
            }
        }

        CoUninitialize();
    })
}

unsafe fn adjust_process_volume(
    step_db: f32,
    direction: WheelDirection,
) -> windows::core::Result<()> {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
    let device_count = devices.GetCount()?;

    for device_index in 0..device_count {
        let Ok(device) = devices.Item(device_index) else {
            continue;
        };
        let _ = adjust_device_sessions(&device, step_db, direction);
    }

    Ok(())
}

unsafe fn adjust_device_sessions(
    device: &IMMDevice,
    step_db: f32,
    direction: WheelDirection,
) -> windows::core::Result<()> {
    let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
    let sessions = manager.GetSessionEnumerator()?;

    let count = sessions.GetCount()?;
    for index in 0..count {
        adjust_session_if_target(&sessions, index, step_db, direction);
    }

    Ok(())
}

unsafe fn adjust_session_if_target(
    sessions: &IAudioSessionEnumerator,
    index: i32,
    step_db: f32,
    direction: WheelDirection,
) {
    let Ok(control) = sessions.GetSession(index) else {
        return;
    };
    let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
        return;
    };
    let Ok(pid) = control2.GetProcessId() else {
        return;
    };

    if !process_name_matches(pid) {
        return;
    }

    let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
        return;
    };
    let Ok(current) = volume.GetMasterVolume() else {
        return;
    };

    // Session volume is exposed as a scalar; applying a dB step means multiplying
    // by the corresponding amplitude ratio.
    let ratio = 10.0_f32.powf(step_db / 20.0);
    let next = match direction {
        WheelDirection::Up => (current * ratio).min(1.0),
        WheelDirection::Down => (current / ratio).max(0.0),
    };

    let _ = volume.SetMasterVolume(next, std::ptr::null());
}

unsafe fn process_name_matches(pid: u32) -> bool {
    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return false;
    };
    if handle.is_invalid() {
        return false;
    }

    let result = query_process_image_name(handle).and_then(|path| {
        path.file_name()
            .map(|name| name.eq_ignore_ascii_case(TARGET_PROCESS))
    });

    let _ = CloseHandle(handle);
    result.unwrap_or(false)
}

unsafe fn query_process_image_name(handle: HANDLE) -> Option<std::path::PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    if QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(buffer.as_mut_ptr()),
        &mut size,
    )
    .is_err()
    {
        return None;
    }

    buffer.truncate(size as usize);
    let value = OsString::from_wide(&buffer);
    Some(std::path::PathBuf::from(value))
}
