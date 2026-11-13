//! Read-only mouse (and optionally keyboard) input through Raw Input.
//!
//! Raw Input only *receives copies* of input events. It is not part of any
//! application's input chain, so it cannot block, change or inject input.
//! Nothing is loaded into other processes. (The optional scroll blocker in
//! `blocker.rs` is the only part that uses a hook, and only while enabled.)

use std::mem::size_of;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_XBUTTON1,
    VK_XBUTTON2,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};

use crate::config::Modifier;

const USAGE_PAGE_GENERIC: u16 = 0x01;
const USAGE_MOUSE: u16 = 0x02;
const USAGE_KEYBOARD: u16 = 0x06;
const RI_MOUSE_WHEEL: u16 = 0x0400;
const RI_MOUSE_BUTTON_4_DOWN: u16 = 0x0040;
const RI_MOUSE_BUTTON_4_UP: u16 = 0x0080;
const RI_MOUSE_BUTTON_5_DOWN: u16 = 0x0100;
const RI_MOUSE_BUTTON_5_UP: u16 = 0x0200;
const RI_KEY_BREAK: u16 = 0x0001;
const WHEEL_NOTCH: i32 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// Subscribes `hwnd` to mouse raw input, also while other apps have focus.
pub fn register(hwnd: HWND) -> windows::core::Result<()> {
    set_device(USAGE_MOUSE, Some(hwnd))
}

/// Turns keyboard raw input on or off. It is only used to notice when the
/// modifier key goes down or up (for "Block scroll"); keys are not stored.
pub fn set_keyboard(hwnd: HWND, on: bool) {
    let _ = set_device(USAGE_KEYBOARD, on.then_some(hwnd));
}

pub fn unregister() {
    let _ = set_device(USAGE_KEYBOARD, None);
    let _ = set_device(USAGE_MOUSE, None);
}

/// `Some(hwnd)` subscribes, `None` unsubscribes.
fn set_device(usage: u16, hwnd: Option<HWND>) -> windows::core::Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: USAGE_PAGE_GENERIC,
        usUsage: usage,
        dwFlags: if hwnd.is_some() {
            RIDEV_INPUTSINK
        } else {
            RIDEV_REMOVE
        },
        hwndTarget: hwnd.unwrap_or_default(),
    };
    unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
}

/// One raw input event, reduced to what Scrollify needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEvent {
    Mouse {
        wheel: Option<i32>,
        /// `Some(true)` on press, `Some(false)` on release.
        x1: Option<bool>,
        x2: Option<bool>,
    },
    Key {
        vk: u16,
        down: bool,
    },
}

/// Reads a `WM_INPUT` message.
pub fn read(lparam: LPARAM) -> Option<RawEvent> {
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
    if read == u32::MAX || read == 0 {
        return None;
    }
    if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
        let key = unsafe { raw.data.keyboard };
        return Some(RawEvent::Key {
            vk: key.VKey,
            down: key.Flags & RI_KEY_BREAK == 0,
        });
    }
    if raw.header.dwType != RIM_TYPEMOUSE.0 {
        return None;
    }
    let buttons = unsafe { raw.data.mouse.Anonymous.Anonymous };
    let flags = buttons.usButtonFlags;
    let state = |down: u16, up: u16| {
        if flags & down != 0 {
            Some(true)
        } else if flags & up != 0 {
            Some(false)
        } else {
            None
        }
    };
    Some(RawEvent::Mouse {
        wheel: (flags & RI_MOUSE_WHEEL != 0).then_some(buttons.usButtonData as i16 as i32),
        x1: state(RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP),
        x2: state(RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP),
    })
}

/// Whether `event` says the modifier went down (`Some(true)`) or up
/// (`Some(false)`). `None` if the event is about something else.
pub fn modifier_change(event: RawEvent, modifier: Modifier) -> Option<bool> {
    match (event, modifier) {
        (RawEvent::Key { vk, down }, _) => {
            let matches = match modifier {
                Modifier::Alt => vk == VK_MENU.0,
                Modifier::Ctrl => vk == VK_CONTROL.0,
                Modifier::Shift => vk == VK_SHIFT.0,
                Modifier::Win => vk == VK_LWIN.0 || vk == VK_RWIN.0,
                Modifier::Mouse4 | Modifier::Mouse5 => false,
            };
            matches.then_some(down)
        }
        (RawEvent::Mouse { x1, .. }, Modifier::Mouse4) => x1,
        (RawEvent::Mouse { x2, .. }, Modifier::Mouse5) => x2,
        _ => None,
    }
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
    fn modifier_changes_are_detected() {
        let alt_down = RawEvent::Key {
            vk: 0x12,
            down: true,
        };
        assert_eq!(modifier_change(alt_down, Modifier::Alt), Some(true));
        assert_eq!(modifier_change(alt_down, Modifier::Ctrl), None);
        let x1_up = RawEvent::Mouse {
            wheel: None,
            x1: Some(false),
            x2: None,
        };
        assert_eq!(modifier_change(x1_up, Modifier::Mouse4), Some(false));
        assert_eq!(modifier_change(x1_up, Modifier::Mouse5), None);
    }

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
