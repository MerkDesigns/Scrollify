//! Settings panel opened from the tray icon.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_HIDE, SW_SHOW};

use super::d2d::{Align, Canvas, Graphics, Rect, Surface, TextStyle};
use super::{monitor_at, theme};
use crate::audio::display_name;
use crate::config::{Modifier, MAX_STEP_DB, MIN_STEP_DB};

const W: f32 = 300.0;
const PAD: f32 = 16.0;
const HEADER_H: f32 = 58.0;
const ROW_H: f32 = 44.0;
const SECTION_H: f32 = 38.0;
const ITEM_H: f32 = 30.0;
const SUBHEAD_H: f32 = 24.0;
const QUIT_H: f32 = 36.0;
const MARGIN: i32 = 8;

const TITLE: TextStyle = TextStyle::new(15.5, 700, Align::Left);
const LABEL: TextStyle = TextStyle::new(13.0, 500, Align::Left);
const SMALL: TextStyle = TextStyle::new(11.5, 600, Align::Left);
const VALUE: TextStyle = TextStyle::new(12.5, 600, Align::Center);
const GLYPH: TextStyle = TextStyle::new(16.0, 400, Align::Center);
const PILL: TextStyle = TextStyle::new(11.5, 700, Align::Left);
const BUTTON: TextStyle = TextStyle::new(13.0, 600, Align::Center);

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    ToggleEnabled,
    ToggleStartup,
    ToggleOverlay,
    ToggleBlock,
    StepDown,
    StepUp,
    ModifierPrev,
    ModifierNext,
    ToggleAddList,
    RemoveTarget(usize),
    AddTarget(String),
    Quit,
}

/// What the panel shows. Built by the app for each frame.
pub struct View<'a> {
    pub enabled: bool,
    pub start_with_windows: bool,
    pub show_overlay: bool,
    pub block_scroll: bool,
    pub step_db: f32,
    pub modifier: Modifier,
    pub targets: &'a [String],
    pub adding: bool,
    /// Apps playing audio that are not targets yet (only used while adding).
    pub available: &'a [String],
    pub apps_loaded: bool,
}

pub struct Panel {
    pub hwnd: HWND,
    surface: Surface,
    scale: f32,
    pos: POINT,
    /// Bottom edge in pixels. Kept fixed when the panel grows or shrinks.
    bottom: i32,
    work: RECT,
    hits: Vec<(Rect, Action)>,
    hover: Option<Action>,
    visible: bool,
    hidden_at: Option<Instant>,
}

impl Panel {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            surface: Surface::new(),
            scale: 1.0,
            pos: POINT::default(),
            bottom: 0,
            work: RECT::default(),
            hits: Vec::new(),
            hover: None,
            visible: false,
            hidden_at: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// True if the panel was closed a moment ago, e.g. by the same tray click
    /// that is now asking to toggle it. Then the click should just close it.
    pub fn just_hidden(&self) -> bool {
        self.hidden_at
            .is_some_and(|t| t.elapsed() < Duration::from_millis(350))
    }

    pub fn show(&mut self, gfx: &Graphics, cursor: POINT, view: &View) {
        let (work, scale) = monitor_at(cursor);
        self.work = work;
        self.scale = scale;
        self.hover = None;
        let h_px = (content_height(view) * scale).ceil() as i32;
        let w_px = (W * scale).ceil() as i32;
        self.pos.x = (cursor.x - w_px + MARGIN)
            .min(work.right - w_px - MARGIN)
            .max(work.left + MARGIN);
        self.bottom = (cursor.y - MARGIN)
            .min(work.bottom - MARGIN)
            .max(work.top + MARGIN + h_px);
        self.render(gfx, view);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
        }
        self.visible = true;
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.hidden_at = Some(Instant::now());
        self.hover = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn render(&mut self, gfx: &Graphics, view: &View) {
        let h = content_height(view);
        let h_px = (h * self.scale).ceil() as i32;
        self.pos.y = (self.bottom - h_px).max(self.work.top + MARGIN);
        let mut hits = Vec::new();
        let hover = self.hover.clone();
        self.surface
            .draw(gfx, self.hwnd, self.pos, (W, h), self.scale, |c| {
                paint(c, view, h, hover.as_ref(), &mut hits)
            });
        self.hits = hits;
    }

    /// Mouse position in window pixels → the action under it.
    pub fn hit(&self, x: i32, y: i32) -> Option<Action> {
        let (x, y) = (x as f32 / self.scale, y as f32 / self.scale);
        self.hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, a)| a.clone())
    }

    /// Updates the hover state. Returns true when a redraw is needed.
    pub fn set_hover(&mut self, action: Option<Action>) -> bool {
        if self.hover == action {
            return false;
        }
        self.hover = action;
        true
    }
}

fn content_height(view: &View) -> f32 {
    let targets = view.targets.len().max(1) as f32 * ITEM_H;
    let adding = if view.adding {
        SUBHEAD_H + view.available.len().max(1) as f32 * ITEM_H
    } else {
        0.0
    };
    HEADER_H + 5.0 * ROW_H + SECTION_H + targets + adding + 12.0 + QUIT_H + PAD
}

fn paint(
    c: &mut Canvas,
    view: &View,
    height: f32,
    hover: Option<&Action>,
    hits: &mut Vec<(Rect, Action)>,
) {
    let is_hover = |a: &Action| hover == Some(a);
    let full = Rect::new(0.0, 0.0, W, height);
    c.fill_round(full, 10.0, theme::BG);
    c.stroke_round(full, 10.0, theme::BORDER, 1.0);

    // Header: logo, name, on/off pill.
    c.logo(Rect::new(PAD, 14.0, 30.0, 30.0));
    c.text(
        "SCROLLIFY",
        Rect::new(PAD + 40.0, 14.0, 150.0, 30.0),
        TITLE,
        theme::TEXT,
    );
    let pill = Rect::new(W - PAD - 70.0, 15.0, 70.0, 28.0);
    let (pill_bg, pill_border, dot, label) = if view.enabled {
        (theme::ACCENT_BG, theme::ACCENT_BORDER, theme::ACCENT, "ON")
    } else {
        (
            theme::CONTROL,
            theme::CONTROL_BORDER,
            theme::TEXT_FAINT,
            "OFF",
        )
    };
    let pill_bg = if is_hover(&Action::ToggleEnabled) {
        theme::CONTROL_HOVER
    } else {
        pill_bg
    };
    c.fill_round(pill, 14.0, pill_bg);
    c.stroke_round(pill, 14.0, pill_border, 1.0);
    if view.enabled {
        c.fill_circle(
            pill.x + 20.0,
            pill.y + 14.0,
            7.5,
            theme::rgba(0x8bed45, 0.25),
        );
    }
    c.fill_circle(pill.x + 20.0, pill.y + 14.0, 4.5, dot);
    c.text(
        label,
        Rect::new(pill.x + 32.0, pill.y, 36.0, 28.0),
        PILL,
        theme::TEXT,
    );
    hits.push((pill, Action::ToggleEnabled));

    divider(c, HEADER_H - 1.0);
    let mut y = HEADER_H;

    // Switch rows.
    for (label, note, on, action) in [
        (
            "Start with Windows",
            None,
            view.start_with_windows,
            Action::ToggleStartup,
        ),
        (
            "Volume popup",
            None,
            view.show_overlay,
            Action::ToggleOverlay,
        ),
        (
            "Block scroll in apps",
            Some("Uses a mouse hook while the key is held"),
            view.block_scroll,
            Action::ToggleBlock,
        ),
    ] {
        let row = Rect::new(PAD, y, W - 2.0 * PAD, ROW_H);
        let color = if is_hover(&action) {
            theme::KNOB_ON
        } else {
            theme::TEXT
        };
        if let Some(note) = note {
            c.text(label, Rect::new(row.x, y + 4.0, 200.0, 20.0), LABEL, color);
            let note_rect = Rect::new(row.x, y + 22.0, 210.0, 16.0);
            c.text(note, note_rect, NOTE, theme::TEXT_FAINT);
        } else {
            c.text(label, Rect::new(row.x, y, 180.0, ROW_H), LABEL, color);
        }
        switch(c, Rect::new(W - PAD - 40.0, y + 11.0, 40.0, 22.0), on);
        hits.push((row, action));
        y += ROW_H;
    }

    // Stepper rows.
    let step_text = format!("{:.1} dB", view.step_db);
    let rows = [
        (
            "Volume step",
            step_text.as_str(),
            (Action::StepDown, view.step_db > MIN_STEP_DB, "\u{2212}"),
            (Action::StepUp, view.step_db < MAX_STEP_DB, "+"),
        ),
        (
            "Hold while scrolling",
            view.modifier.label(),
            (Action::ModifierPrev, true, "\u{2039}"),
            (Action::ModifierNext, true, "\u{203A}"),
        ),
    ];
    for (label, value, (left, left_on, left_glyph), (right, right_on, right_glyph)) in rows {
        c.text(label, Rect::new(PAD, y, 150.0, ROW_H), LABEL, theme::TEXT);
        let right_btn = Rect::new(W - PAD - 28.0, y + 8.0, 28.0, 28.0);
        let value_box = Rect::new(right_btn.x - 4.0 - 66.0, y + 8.0, 66.0, 28.0);
        let left_btn = Rect::new(value_box.x - 4.0 - 28.0, y + 8.0, 28.0, 28.0);
        button(c, left_btn, left_glyph, GLYPH, left_on, is_hover(&left));
        control(c, value_box, false);
        c.text(value, value_box, VALUE, theme::TEXT);
        button(c, right_btn, right_glyph, GLYPH, right_on, is_hover(&right));
        if left_on {
            hits.push((left_btn, left));
        }
        if right_on {
            hits.push((right_btn, right));
        }
        y += ROW_H;
    }

    divider(c, y);

    // Target apps.
    c.text(
        "APPS",
        Rect::new(PAD, y, 100.0, SECTION_H),
        SMALL,
        theme::TEXT_DIM,
    );
    let add = Rect::new(W - PAD - 64.0, y + 8.0, 64.0, 24.0);
    let add_label = if view.adding { "Done" } else { "+ Add" };
    button(
        c,
        add,
        add_label,
        SMALL_CENTER,
        true,
        is_hover(&Action::ToggleAddList),
    );
    hits.push((add, Action::ToggleAddList));
    y += SECTION_H;

    if view.targets.is_empty() {
        let note = "No apps. Add one to get started.";
        c.text(
            note,
            Rect::new(PAD, y, W - 2.0 * PAD, ITEM_H),
            LABEL,
            theme::TEXT_FAINT,
        );
        y += ITEM_H;
    }
    for (i, exe) in view.targets.iter().enumerate() {
        let action = Action::RemoveTarget(i);
        c.fill_circle(PAD + 4.0, y + ITEM_H / 2.0, 3.0, theme::ACCENT);
        let name_rect = Rect::new(PAD + 16.0, y, W - 2.0 * PAD - 48.0, ITEM_H);
        c.text(display_name(exe), name_rect, LABEL, theme::TEXT);
        let remove = Rect::new(W - PAD - 24.0, y + 3.0, 24.0, 24.0);
        let hovered = is_hover(&action);
        if hovered {
            c.fill_round(remove, 6.0, theme::CONTROL_HOVER);
        }
        let color = if hovered {
            theme::DANGER
        } else {
            theme::TEXT_FAINT
        };
        c.text("\u{00D7}", remove, GLYPH, color);
        hits.push((remove, action));
        y += ITEM_H;
    }

    if view.adding {
        let heading = if view.apps_loaded {
            "PLAYING AUDIO NOW"
        } else {
            "LOOKING FOR APPS\u{2026}"
        };
        c.text(
            heading,
            Rect::new(PAD, y, 200.0, SUBHEAD_H),
            SMALL,
            theme::TEXT_DIM,
        );
        y += SUBHEAD_H;
        if view.available.is_empty() {
            let note = if view.apps_loaded {
                "No other apps are playing audio."
            } else {
                ""
            };
            c.text(
                note,
                Rect::new(PAD, y, W - 2.0 * PAD, ITEM_H),
                LABEL,
                theme::TEXT_FAINT,
            );
            y += ITEM_H;
        }
        for exe in view.available {
            let action = Action::AddTarget(exe.clone());
            let row = Rect::new(PAD - 6.0, y + 1.0, W - 2.0 * PAD + 12.0, ITEM_H - 2.0);
            let hovered = is_hover(&action);
            if hovered {
                c.fill_round(row, 6.0, theme::CONTROL_HOVER);
            }
            let name_rect = Rect::new(PAD + 16.0, y, W - 2.0 * PAD - 48.0, ITEM_H);
            c.text(display_name(exe), name_rect, LABEL, theme::TEXT_DIM);
            let plus = Rect::new(W - PAD - 24.0, y + 3.0, 24.0, 24.0);
            let color = if hovered {
                theme::ACCENT
            } else {
                theme::TEXT_FAINT
            };
            c.text("+", plus, GLYPH, color);
            hits.push((row, action));
            y += ITEM_H;
        }
    }

    // Quit.
    y += 12.0;
    let quit = Rect::new(PAD, y, W - 2.0 * PAD, QUIT_H);
    button(c, quit, "Quit", BUTTON, true, is_hover(&Action::Quit));
    hits.push((quit, Action::Quit));
}

const NOTE: TextStyle = TextStyle::new(10.5, 400, Align::Left);
const SMALL_CENTER: TextStyle = TextStyle::new(11.5, 600, Align::Center);

fn divider(c: &mut Canvas, y: f32) {
    c.line((PAD, y + 0.5), (W - PAD, y + 0.5), theme::DIVIDER, 1.0);
}

fn control(c: &mut Canvas, r: Rect, hovered: bool) {
    let bg = if hovered {
        theme::CONTROL_HOVER
    } else {
        theme::CONTROL
    };
    let border = if hovered {
        theme::ACCENT_BORDER
    } else {
        theme::CONTROL_BORDER
    };
    c.fill_round(r, 7.0, bg);
    c.stroke_round(r, 7.0, border, 1.0);
}

fn button(c: &mut Canvas, r: Rect, label: &str, style: TextStyle, enabled: bool, hovered: bool) {
    control(c, r, enabled && hovered);
    let color = match (enabled, hovered) {
        (false, _) => theme::TEXT_FAINT,
        (true, true) => theme::ACCENT,
        (true, false) => theme::TEXT,
    };
    c.text(label, r, style, color);
}

fn switch(c: &mut Canvas, r: Rect, on: bool) {
    let radius = r.h / 2.0;
    let (bg, border, knob, knob_x) = if on {
        (
            theme::SWITCH_ON,
            theme::ACCENT_BORDER,
            theme::KNOB_ON,
            r.x + r.w - radius,
        )
    } else {
        (
            theme::CONTROL,
            theme::CONTROL_BORDER,
            theme::KNOB_OFF,
            r.x + radius,
        )
    };
    c.fill_round(r, radius, bg);
    c.stroke_round(r, radius, border, 1.0);
    c.fill_circle(knob_x, r.y + radius, radius - 4.0, knob);
}
