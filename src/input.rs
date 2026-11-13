//! Read-only mouse wheel input through Raw Input.
//!
//! Raw Input only *receives copies* of mouse events. It is not part of any
//! application's input chain, so it cannot block, change or inject input.
//! There is no global hook and nothing is loaded into other processes.

use std::mem::size_of;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_XBUTTON1,
    VK_XBUTTON2,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEMOUSE,
};

use crate::config::Modifier;

const USAGE_PAGE_GENERIC: u16 = 0x01;
const USAGE_MOUSE: u16 = 0x02;
const RI_MOUSE_WHEEL: u16 = 0x0400;
const WHEEL_NOTCH: i32 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// Subscribes `hwnd` to mouse raw input, also while other apps have focus.
pub fn register(hwnd: HWND) -> windows::core::Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: USAGE_PAGE_GENERIC,
        usUsage: USAGE_MOUSE,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
}

pub fn unregister() {
    let device = RAWINPUTDEVICE {
        usUsagePage: USAGE_PAGE_GENERIC,
        usUsage: USAGE_MOUSE,
        dwFlags: RIDEV_REMOVE,
        hwndTarget: HWND::default(),
    };
    unsafe {
        let _ = RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32);
    }
}

/// Reads the vertical wheel delta from a `WM_INPUT` message, if it has one.
pub fn wheel_delta(lparam: LPARAM) -> Option<i32> {
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    let read = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as *mut _),
            RID_INPUT,
            Some(&mut raw as *mut RAWINPUT as *mut _),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || read == 0 || raw.header.dwType != RIM_TYPEMOUSE.0 {
        return None;
    }
    let buttons = unsafe { raw.data.mouse.Anonymous.Anonymous };
    if buttons.usButtonFlags & RI_MOUSE_WHEEL == 0 {
        return None;
    }
    Some(buttons.usButtonData as i16 as i32)
}

/// True while the chosen key or mouse button is held. Only reads key state.
pub fn modifier_held(modifier: Modifier) -> bool {
    let down = |key: VIRTUAL_KEY| unsafe { (GetAsyncKeyState(key.0 as i32) as u16 & 0x8000) != 0 };
    match modifier {
        Modifier::Alt => down(VK_MENU),
        Modifier::Ctrl => down(VK_CONTROL),
        Modifier::Shift => down(VK_SHIFT),
        Modifier::Win => down(VK_LWIN) || down(VK_RWIN),
        Modifier::Mouse4 => down(VK_XBUTTON1),
        Modifier::Mouse5 => down(VK_XBUTTON2),
    }
}

/// Turns raw wheel deltas into whole notches. High-resolution wheels and
/// touchpads send many small deltas; this keeps one step per 120 units.
#[derive(Default)]
pub struct WheelAccumulator {
    pending: i32,
}

impl WheelAccumulator {
    pub fn push(&mut self, delta: i32) -> Vec<Direction> {
        if delta == 0 {
            return Vec::new();
        }
        // Changing direction drops leftovers from the other direction.
        if self.pending != 0 && self.pending.signum() != delta.signum() {
            self.pending = 0;
        }
        self.pending += delta;
        let mut steps = Vec::new();
        while self.pending >= WHEEL_NOTCH {
            self.pending -= WHEEL_NOTCH;
            steps.push(Direction::Up);
        }
        while self.pending <= -WHEEL_NOTCH {
            self.pending += WHEEL_NOTCH;
            steps.push(Direction::Down);
        }
        steps
    }

    pub fn reset(&mut self) {
        self.pending = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_notches_map_to_steps() {
        let mut acc = WheelAccumulator::default();
        assert_eq!(acc.push(120), vec![Direction::Up]);
        assert_eq!(acc.push(-240), vec![Direction::Down, Direction::Down]);
    }

    #[test]
    fn small_deltas_add_up() {
        let mut acc = WheelAccumulator::default();
        assert!(acc.push(40).is_empty());
        assert!(acc.push(40).is_empty());
        assert_eq!(acc.push(40), vec![Direction::Up]);
    }

    #[test]
    fn direction_change_drops_leftovers() {
        let mut acc = WheelAccumulator::default();
        assert!(acc.push(100).is_empty());
        assert!(acc.push(-100).is_empty());
        assert_eq!(acc.push(-20), vec![Direction::Down]);
    }
}
