//! Notification-area icon.

use std::mem::size_of;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GetSystemMetrics, HICON, LR_DEFAULTCOLOR, SM_CXSMICON,
    SM_CYSMICON,
};

const ICON_ON: &[u8] = include_bytes!("../Assets/ScrollifyTrayOn.ico");
const ICON_OFF: &[u8] = include_bytes!("../Assets/ScrollifyTrayOff.ico");
const TRAY_ID: u32 = 1;

pub struct Tray {
    hwnd: HWND,
    callback_msg: u32,
    on: Option<HICON>,
    off: Option<HICON>,
}

impl Tray {
    pub fn new(hwnd: HWND, callback_msg: u32, enabled: bool) -> Self {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
        let tray = Self {
            hwnd,
            callback_msg,
            on: icon_from_ico(ICON_ON, w, h),
            off: icon_from_ico(ICON_OFF, w, h),
        };
        tray.add(enabled);
        tray
    }

    /// Adds the icon. Also used after Explorer restarts (`TaskbarCreated`).
    pub fn add(&self, enabled: bool) {
        let mut data = self.data(enabled);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = self.callback_msg;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut data = self.data(enabled);
        data.uFlags = NIF_ICON | NIF_TIP;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    fn data(&self, enabled: bool) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            hIcon: if enabled { self.on } else { self.off }.unwrap_or_default(),
            ..Default::default()
        };
        let tip = if enabled {
            "Scrollify: on"
        } else {
            "Scrollify: off"
        };
        for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16()) {
            *slot = unit;
        }
        data
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = self.data(false);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            for icon in [self.on, self.off].into_iter().flatten() {
                let _ = DestroyIcon(icon);
            }
        }
    }
}

/// Picks the image in an `.ico` file closest to the wanted size (more colour
/// depth breaks ties) and turns it into an icon handle.
fn icon_from_ico(bytes: &[u8], want_w: i32, want_h: i32) -> Option<HICON> {
    let (offset, len) = best_ico_entry(bytes, want_w, want_h)?;
    unsafe {
        CreateIconFromResourceEx(
            &bytes[offset..offset + len],
            true,
            0x0003_0000,
            want_w,
            want_h,
            LR_DEFAULTCOLOR,
        )
        .ok()
    }
}

fn best_ico_entry(bytes: &[u8], want_w: i32, want_h: i32) -> Option<(usize, usize)> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));

    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    let count = u16_at(4)? as usize;
    let mut best: Option<((i32, i32), (usize, usize))> = None;
    for i in 0..count {
        let entry = 6 + i * 16;
        let dim = |b: u8| if b == 0 { 256 } else { b as i32 };
        let w = dim(*bytes.get(entry)?);
        let h = dim(*bytes.get(entry + 1)?);
        let bits = u16_at(entry + 6)? as i32;
        let len = u32_at(entry + 8)? as usize;
        let offset = u32_at(entry + 12)? as usize;
        if offset.checked_add(len)? > bytes.len() {
            continue;
        }
        // Lower distance is better, then higher bit depth.
        let score = ((w - want_w).abs() + (h - want_h).abs(), -bits);
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, (offset, len)));
        }
    }
    best.map(|(_, entry)| entry)
}
