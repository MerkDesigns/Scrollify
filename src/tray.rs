use std::mem::size_of;
use std::sync::Arc;

use crate::app_state::AppState;
use crate::settings_ui::SettingsUiHandle;
use crate::util::{loword, to_wide};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow,
    GetCursorPos, GetSystemMetrics, LoadIconW, RegisterClassW, HICON, HWND_MESSAGE,
    IDI_APPLICATION, LR_DEFAULTCOLOR, SM_CXSMICON, SM_CYSMICON, WM_APP, WM_CREATE, WM_DESTROY,
    WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_EX_NOACTIVATE,
};

const WM_TRAYICON: u32 = WM_APP + 1;
const ID_TRAY: u32 = 1;
const ICON_RESOURCE_VERSION: u32 = 0x0003_0000;
const TRAY_ON_ICON: &[u8] = include_bytes!("../Assets/ScrollifyTrayOn.ico");
const TRAY_OFF_ICON: &[u8] = include_bytes!("../Assets/ScrollifyTrayOff.ico");

struct TrayContext {
    state: Arc<AppState>,
    settings_ui: SettingsUiHandle,
}

pub struct TrayApp {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    state: Arc<AppState>,
}

impl TrayApp {
    pub fn new(state: Arc<AppState>, settings_ui: SettingsUiHandle) -> windows::core::Result<Self> {
        unsafe {
            let enabled = state.settings().enabled;
            let context = Arc::new(TrayContext {
                state: state.clone(),
                settings_ui,
            });
            let instance = GetModuleHandleW(None)?;
            let class_name = to_wide("Scrollify.TrayWindow");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(to_wide("Scrollify").as_ptr()),
                Default::default(),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                instance,
                Some(Arc::into_raw(context) as *const _),
            )?;

            let icon = load_tray_icon(enabled)
                .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                .ok_or_else(windows::core::Error::from_win32)?;

            let mut nid = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: ID_TRAY,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: icon,
                ..Default::default()
            };
            write_tip(&mut nid, "Scrollify");
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                destroy_owned_icon(icon);
                return Err(windows::core::Error::from_win32());
            }
            destroy_owned_icon(icon);

            Ok(Self { hwnd, nid, state })
        }
    }

    pub fn refresh_icon(&self) {
        unsafe {
            update_icon(self.hwnd, self.state.settings().enabled);
        }
    }
}

impl Drop for TrayApp {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn write_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
    let tip = to_wide(tip);
    let len = tip.len().min(nid.szTip.len());
    nid.szTip[..len].copy_from_slice(&tip[..len]);
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let createstruct =
                &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                createstruct.lpCreateParams as isize,
            );
            LRESULT(0)
        }
        WM_TRAYICON => {
            let mouse_msg = loword(lparam.0 as usize) as u32;
            if let Some(context) = context_from_hwnd(hwnd) {
                if mouse_msg == WM_LBUTTONUP {
                    let enabled = !context.state.settings().enabled;
                    context.state.set_enabled(enabled);
                    update_icon(hwnd, enabled);
                } else if mouse_msg == WM_RBUTTONDOWN {
                    context.settings_ui.begin_tray_right_click();
                } else if mouse_msg == WM_RBUTTONUP {
                    let mut point = POINT::default();
                    let _ = GetCursorPos(&mut point);
                    context.settings_ui.toggle(point);
                    update_icon(hwnd, context.state.settings().enabled);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            ) as *const TrayContext;
            if !raw.is_null() {
                drop(Arc::from_raw(raw));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn update_icon(hwnd: HWND, enabled: bool) {
    let Some(icon) = load_tray_icon(enabled) else {
        return;
    };

    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ID_TRAY,
        uFlags: NIF_ICON,
        hIcon: icon,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    destroy_owned_icon(icon);
}

unsafe fn load_tray_icon(enabled: bool) -> Option<HICON> {
    let width = GetSystemMetrics(SM_CXSMICON);
    let height = GetSystemMetrics(SM_CYSMICON);
    let bytes = if enabled { TRAY_ON_ICON } else { TRAY_OFF_ICON };
    load_icon_from_ico(bytes, width, height)
}

unsafe fn load_icon_from_ico(
    bytes: &[u8],
    desired_width: i32,
    desired_height: i32,
) -> Option<HICON> {
    if bytes.len() < 6 || read_u16(bytes, 0)? != 0 || read_u16(bytes, 2)? != 1 {
        return None;
    }

    let count = read_u16(bytes, 4)? as usize;
    let mut best: Option<(i32, u16, usize, usize)> = None;

    for index in 0..count {
        let offset = 6 + index * 16;
        if offset + 16 > bytes.len() {
            return None;
        }

        let width = icon_dimension(bytes[offset]);
        let height = icon_dimension(bytes[offset + 1]);
        let bit_count = read_u16(bytes, offset + 6).unwrap_or_default();
        let image_size = read_u32(bytes, offset + 8)? as usize;
        let image_offset = read_u32(bytes, offset + 12)? as usize;
        if image_offset.checked_add(image_size)? > bytes.len() {
            continue;
        }

        let score = (width - desired_width).abs() + (height - desired_height).abs();
        let candidate = (score, bit_count, image_offset, image_size);
        if best.as_ref().is_none_or(|current| {
            score < current.0 || (score == current.0 && bit_count > current.1)
        }) {
            best = Some(candidate);
        }
    }

    let (_, _, image_offset, image_size) = best?;
    CreateIconFromResourceEx(
        &bytes[image_offset..image_offset + image_size],
        true,
        ICON_RESOURCE_VERSION,
        desired_width,
        desired_height,
        LR_DEFAULTCOLOR,
    )
    .ok()
}

fn icon_dimension(value: u8) -> i32 {
    if value == 0 {
        256
    } else {
        value as i32
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

unsafe fn destroy_owned_icon(icon: HICON) {
    if !icon.is_invalid() {
        let _ = DestroyIcon(icon);
    }
}

unsafe fn context_from_hwnd(hwnd: HWND) -> Option<Arc<TrayContext>> {
    let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
        hwnd,
        windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
    ) as *const TrayContext;
    if raw.is_null() {
        return None;
    }

    Arc::increment_strong_count(raw);
    Some(Arc::from_raw(raw))
}
