//! Optional: stops modifier + wheel from also scrolling the app under the
//! mouse. Off by default.
//!
//! Blocking input is only possible with a low-level mouse hook, so this is the
//! one feature that uses one. To keep its footprint small, the hook is
//! installed only while the modifier is held and removed as soon as it is
//! released. It only swallows vertical wheel events while the modifier is
//! held; every other mouse event passes straight through.

use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicUsize, Ordering};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
    MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEWHEEL,
};

use crate::config::Modifier;
use crate::input;

static NOTIFY_HWND: AtomicIsize = AtomicIsize::new(0);
static NOTIFY_MSG: AtomicU32 = AtomicU32::new(0);
static MODIFIER: AtomicUsize = AtomicUsize::new(0);

pub struct Blocker {
    hook: Option<HHOOK>,
}

impl Blocker {
    /// Blocked wheel deltas are posted to `hwnd` as `msg` (`wparam` = delta).
    pub fn new(hwnd: HWND, msg: u32) -> Self {
        NOTIFY_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        NOTIFY_MSG.store(msg, Ordering::Relaxed);
        Self { hook: None }
    }

    pub fn is_active(&self) -> bool {
        self.hook.is_some()
    }

    pub fn start(&mut self, modifier: Modifier) {
        let index = Modifier::ALL
            .iter()
            .position(|m| *m == modifier)
            .unwrap_or(0);
        MODIFIER.store(index, Ordering::Relaxed);
        if self.hook.is_none() {
            self.hook = unsafe {
                SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), HINSTANCE::default(), 0).ok()
            };
        }
    }

    pub fn stop(&mut self) {
        if let Some(hook) = self.hook.take() {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
    }
}

impl Drop for Blocker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Kept tiny: Windows drops hooks that respond slowly.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_MOUSEWHEEL {
        let index = MODIFIER.load(Ordering::Relaxed) % Modifier::ALL.len();
        if input::modifier_held(Modifier::ALL[index]) {
            let event = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let delta = (event.mouseData >> 16) as u16 as i16;
            let hwnd = HWND(NOTIFY_HWND.load(Ordering::Relaxed) as *mut _);
            let msg = NOTIFY_MSG.load(Ordering::Relaxed);
            let _ = PostMessageW(hwnd, msg, WPARAM(delta as isize as usize), LPARAM(0));
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
