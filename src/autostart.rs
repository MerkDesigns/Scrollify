//! Per-user Start Menu and Startup shortcuts. No registry, no admin rights.

use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

const LINK_NAME: &str = "Scrollify.lnk";

/// Creates or refreshes the Start Menu shortcut, and repoints the Startup
/// shortcut at this executable if it already exists (e.g. after moving the exe).
pub fn refresh_shortcuts() {
    if let Some(path) = programs_dir().map(|dir| dir.join(LINK_NAME)) {
        let _ = write_shortcut(&path);
    }
    if is_enabled() {
        set_enabled(true);
    }
}

pub fn is_enabled() -> bool {
    startup_link().is_some_and(|path| path.exists())
}

pub fn set_enabled(enabled: bool) {
    let Some(path) = startup_link() else {
        return;
    };
    if enabled {
        let _ = write_shortcut(&path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

fn programs_dir() -> Option<PathBuf> {
    let mut dir = PathBuf::from(std::env::var_os("APPDATA")?);
    dir.extend(["Microsoft", "Windows", "Start Menu", "Programs"]);
    Some(dir)
}

fn startup_link() -> Option<PathBuf> {
    Some(programs_dir()?.join("Startup").join(LINK_NAME))
}

fn write_shortcut(link: &Path) -> windows::core::Result<()> {
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::from_win32())?;
    if let Some(dir) = link.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell_link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        if let Some(dir) = exe.parent() {
            shell_link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        shell_link.SetDescription(&HSTRING::from("Scroll to change app volume"))?;
        let file: IPersistFile = shell_link.cast()?;
        file.Save(&HSTRING::from(link.as_os_str()), true)
    }
}
