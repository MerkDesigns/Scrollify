use crate::app_state;
use crate::audio::WheelDirection;
use crate::util::hiword;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_MOUSEWHEEL,
};

pub struct MouseHook {
    hook: HHOOK,
}

impl MouseHook {
    pub fn install() -> windows::core::Result<Self> {
        unsafe {
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), HINSTANCE::default(), 0)?;
            Ok(Self { hook })
        }
    }
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.hook);
        }
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_MOUSEWHEEL {
        let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
        if alt_down {
            let event = *(lparam.0 as *const MSLLHOOKSTRUCT);
            let wheel_delta = hiword(event.mouseData as usize) as i16;
            let direction = if wheel_delta > 0 {
                WheelDirection::Up
            } else {
                WheelDirection::Down
            };

            if let Some(state) = app_state::global() {
                state.adjust_target_volume(direction);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}
