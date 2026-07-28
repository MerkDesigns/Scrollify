use std::path::{Path, PathBuf};

use crate::util::to_wide;
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

const SHORTCUT_NAME: &str = "Scrollify.lnk";

pub fn ensure_start_menu_shortcut() {
    if let Some(path) = start_menu_shortcut_path() {
        let _ = create_shortcut(&path);
    }
}

pub fn set_start_with_windows(enabled: bool) {
    let Some(path) = startup_shortcut_path() else {
        return;
    };

    if enabled {
        let _ = create_shortcut(&path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

pub fn is_start_with_windows() -> bool {
    startup_shortcut_path().is_some_and(|path| path.exists())
}

fn start_menu_shortcut_path() -> Option<PathBuf> {
    appdata_path(&["Microsoft", "Windows", "Start Menu", "Programs"])
}

fn startup_shortcut_path() -> Option<PathBuf> {
    appdata_path(&["Microsoft", "Windows", "Start Menu", "Programs", "Startup"])
}

fn appdata_path(parts: &[&str]) -> Option<PathBuf> {
    let mut path = PathBuf::from(std::env::var_os("APPDATA")?);
    path.extend(parts);
    path.push(SHORTCUT_NAME);
    Some(path)
}

fn create_shortcut(shortcut: &Path) -> windows::core::Result<()> {
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::from_win32())?;

    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let exe = to_wide(exe.to_string_lossy().as_ref());
        shell_link.SetPath(PCWSTR(exe.as_ptr()))?;

        let persist_file: IPersistFile = shell_link.cast()?;
        let shortcut = to_wide(shortcut.to_string_lossy().as_ref());
        persist_file.Save(PCWSTR(shortcut.as_ptr()), true)
    }
}
