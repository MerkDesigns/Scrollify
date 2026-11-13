//! Volume popup shown while scrolling. It is a normal top-most window that
//! never takes focus and lets clicks through; nothing is drawn inside other
//! apps. It does not appear over exclusive-fullscreen games.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, KillTimer, SetTimer, SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
};

use super::d2d::{Align, Graphics, Rect, Surface, TextStyle};
use super::{monitor_at, theme};

pub const TIMER_ID: usize = 1;
const W: f32 = 240.0;
const H: f32 = 58.0;
const BOTTOM_GAP: f32 = 72.0;
const HOLD: Duration = Duration::from_millis(1100);
const FADE: Duration = Duration::from_millis(260);

const NAME: TextStyle = TextStyle::new(13.0, 600, Align::Left);
const PERCENT: TextStyle = TextStyle::new(13.0, 700, Align::Right);

pub struct Overlay {
    hwnd: HWND,
    surface: Surface,
    pos: POINT,
    shown_at: Option<Instant>,
}

impl Overlay {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            surface: Surface::new(),
            pos: POINT::default(),
            shown_at: None,
        }
    }

    pub fn show(&mut self, gfx: &Graphics, app: &str, volume: f32) {
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let (work, scale) = monitor_at(cursor);
        let w_px = (W * scale).ceil() as i32;
        let h_px = (H * scale).ceil() as i32;
        self.pos = POINT {
            x: work.left + (work.right - work.left - w_px) / 2,
            y: work.bottom - h_px - (BOTTOM_GAP * scale) as i32,
        };

        let percent = (volume.clamp(0.0, 1.0) * 100.0).round();
        self.surface
            .draw(gfx, self.hwnd, self.pos, (W, H), scale, |c| {
                let full = Rect::new(0.0, 0.0, W, H);
                c.fill_round(full, 14.0, theme::OVERLAY_BG);
                c.stroke_round(full, 14.0, theme::BORDER, 1.0);
                c.text(app, Rect::new(18.0, 8.0, W - 90.0, 26.0), NAME, theme::TEXT);
                let label = if percent == 0.0 {
                    "Muted".to_string()
                } else {
                    format!("{percent:.0}%")
                };
                c.text(
                    &label,
                    Rect::new(W - 78.0, 8.0, 60.0, 26.0),
                    PERCENT,
                    theme::ACCENT,
                );
                let track = Rect::new(18.0, 38.0, W - 36.0, 6.0);
                c.fill_round(track, 3.0, theme::OVERLAY_TRACK);
                if percent > 0.0 {
                    let fill = Rect::new(track.x, track.y, track.w * (percent / 100.0), track.h);
                    c.fill_round(fill, 3.0, theme::ACCENT);
                }
            });

        unsafe {
            if self.shown_at.is_none() {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            // Stay above windows that became top-most after us.
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            SetTimer(self.hwnd, TIMER_ID, 16, None);
        }
        self.shown_at = Some(Instant::now());
    }

    /// Advances the fade-out. Called from the window's timer.
    pub fn tick(&mut self) {
        let Some(shown_at) = self.shown_at else {
            return;
        };
        let elapsed = shown_at.elapsed();
        if elapsed < HOLD {
            return;
        }
        let fade = (elapsed - HOLD).as_secs_f32() / FADE.as_secs_f32();
        if fade >= 1.0 {
            self.hide();
            return;
        }
        let alpha = ((1.0 - fade) * 255.0) as u8;
        self.surface.present(self.hwnd, self.pos, alpha);
    }

    pub fn hide(&mut self) {
        self.shown_at = None;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_ID);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}
