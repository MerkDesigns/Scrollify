//! Colours and fonts for the panel and the volume popup.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub const fn rgb(hex: u32) -> D2D1_COLOR_F {
    rgba(hex, 1.0)
}

pub const fn rgba(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a,
    }
}

pub const FONT: &str = "Segoe UI";

pub const BG: D2D1_COLOR_F = rgb(0x080808);
pub const BORDER: D2D1_COLOR_F = rgb(0x343434);
pub const DIVIDER: D2D1_COLOR_F = rgb(0x242424);
pub const CONTROL: D2D1_COLOR_F = rgb(0x151515);
pub const CONTROL_HOVER: D2D1_COLOR_F = rgb(0x1f2621);
pub const CONTROL_BORDER: D2D1_COLOR_F = rgba(0x60695f, 0.55);
pub const TEXT: D2D1_COLOR_F = rgb(0xeef1ee);
pub const TEXT_DIM: D2D1_COLOR_F = rgb(0x9aa19c);
pub const TEXT_FAINT: D2D1_COLOR_F = rgb(0x68706b);
pub const ACCENT: D2D1_COLOR_F = rgb(0x8bed45);
pub const ACCENT_BG: D2D1_COLOR_F = rgb(0x182415);
pub const ACCENT_BORDER: D2D1_COLOR_F = rgb(0x557b3e);
pub const SWITCH_ON: D2D1_COLOR_F = rgb(0x344b29);
pub const KNOB_OFF: D2D1_COLOR_F = rgb(0xaab0ac);
pub const KNOB_ON: D2D1_COLOR_F = rgb(0xf4fbf0);
pub const DANGER: D2D1_COLOR_F = rgb(0xff6b6b);

pub const OVERLAY_BG: D2D1_COLOR_F = rgba(0x0c0c0c, 0.94);
pub const OVERLAY_TRACK: D2D1_COLOR_F = rgb(0x2a2a2a);
