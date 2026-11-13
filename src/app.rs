//! App state, window procedures and the message loop. Everything here runs on
//! the main thread; only the audio worker has its own thread.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    LoadCursorW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetCursor,
    TranslateMessage, IDC_ARROW, MSG, WA_INACTIVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE,
    WM_APP, WM_INPUT, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONUP,
    WM_SETCURSOR, WM_TIMER, WNDCLASSW, WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::audio::{display_name, AudioWorker, Command, Report};
use crate::autostart;
use crate::blocker::Blocker;
use crate::config::{normalize_step, normalize_targets, ConfigStore, Settings, STEP_INCREMENT_DB};
use crate::input::{self, RawEvent, WheelAccumulator};
use crate::tray::Tray;
use crate::ui::d2d::Graphics;
use crate::ui::overlay::{self, Overlay};
use crate::ui::panel::{Action, Panel, View};

const WM_TRAY: u32 = WM_APP + 1;
const WM_AUDIO: u32 = WM_APP + 2;
const WM_HOOK_WHEEL: u32 = WM_APP + 3;
const VK_ESCAPE: usize = 0x1B;
const WM_MOUSELEAVE: u32 = 0x02A3;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

enum Event {
    Input(RawEvent),
    /// Wheel delta from the scroll blocker's hook.
    HookWheel(i32),
    Tray(u32),
    TaskbarCreated,
    Audio,
    PanelMove(i32, i32),
    PanelLeave,
    PanelClick(i32, i32),
    PanelClose,
    OverlayTick,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// Handles an event, or queues it if the app is already handling one (Win32
/// calls window procedures re-entrantly, e.g. hiding a window sends
/// `WM_ACTIVATE` straight away).
fn dispatch(event: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back(event));
    if BUSY.get() {
        return;
    }
    BUSY.set(true);
    while let Some(event) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
        APP.with(|app| {
            if let Some(app) = app.borrow_mut().as_mut() {
                app.handle(event);
            }
        });
    }
    BUSY.set(false);
}

struct App {
    host: HWND,
    blocker: Blocker,
    gfx: Graphics,
    store: ConfigStore,
    settings: Settings,
    audio: AudioWorker,
    tray: Tray,
    panel: Panel,
    overlay: Overlay,
    wheel: WheelAccumulator,
    start_with_windows: bool,
    adding: bool,
    apps: Vec<String>,
    apps_loaded: bool,
}

impl App {
    fn handle(&mut self, event: Event) {
        match event {
            Event::Input(raw) => self.on_input(raw),
            Event::HookWheel(delta) => self.on_wheel(delta),
            Event::Tray(mouse) => self.on_tray(mouse),
            Event::TaskbarCreated => self.tray.add(self.settings.enabled),
            Event::Audio => self.on_audio(),
            Event::PanelMove(x, y) => {
                let hit = self.panel.hit(x, y);
                if self.panel.set_hover(hit) {
                    self.render_panel();
                }
            }
            Event::PanelLeave => {
                if self.panel.set_hover(None) {
                    self.render_panel();
                }
            }
            Event::PanelClick(x, y) => {
                if let Some(action) = self.panel.hit(x, y) {
                    self.on_action(action);
                }
            }
            Event::PanelClose => self.close_panel(),
            Event::OverlayTick => self.overlay.tick(),
        }
    }

    fn on_input(&mut self, raw: RawEvent) {
        if self.settings.block_scroll {
            let modifier = self.settings.modifier;
            let held = input::modifier_change(raw, modifier)
                .unwrap_or_else(|| input::modifier_held(modifier));
            self.update_blocker(held);
        }
        if let RawEvent::Mouse {
            wheel: Some(delta), ..
        } = raw
        {
            // While the blocker is on, wheel events come from its hook instead.
            if !self.blocker.is_active() {
                self.on_wheel(delta);
            }
        }
    }

    /// Installs the scroll blocker while the modifier is held, removes it otherwise.
    fn update_blocker(&mut self, held: bool) {
        let s = &self.settings;
        let want = held && s.enabled && s.block_scroll && !s.targets.is_empty();
        if want && !self.blocker.is_active() {
            self.blocker.start(s.modifier);
        } else if !want && self.blocker.is_active() {
            self.blocker.stop();
        }
    }

    /// Applies a change to the block setting, the modifier or on/off.
    fn sync_blocker(&mut self) {
        self.blocker.stop();
        input::set_keyboard(self.host, self.settings.block_scroll);
    }

    fn on_wheel(&mut self, delta: i32) {
        if !self.settings.enabled || !input::modifier_held(self.settings.modifier) {
            self.wheel.reset();
            return;
        }
        for direction in self.wheel.push(delta) {
            self.audio.send(Command::Adjust {
                direction,
                step_db: self.settings.volume_step_db,
                targets: self.settings.targets.clone(),
            });
        }
    }

    fn on_tray(&mut self, mouse: u32) {
        match mouse {
            WM_LBUTTONUP => self.set_enabled(!self.settings.enabled),
            WM_RBUTTONUP => {
                if self.panel.is_visible() {
                    self.close_panel();
                } else if !self.panel.just_hidden() {
                    self.open_panel();
                }
            }
            _ => {}
        }
    }

    fn on_audio(&mut self) {
        let mut latest_volume = None;
        while let Some(report) = self.audio.try_report() {
            match report {
                Report::Volume { app, volume } => latest_volume = Some((app, volume)),
                Report::Apps(apps) => {
                    self.apps = apps;
                    self.apps_loaded = true;
                    self.render_panel();
                }
            }
        }
        if let Some((app, volume)) = latest_volume {
            if self.settings.show_overlay {
                self.overlay.show(&self.gfx, display_name(&app), volume);
            }
        }
    }

    fn on_action(&mut self, action: Action) {
        let s = &mut self.settings;
        match action {
            Action::ToggleEnabled => {
                self.set_enabled(!self.settings.enabled);
                self.render_panel();
                return;
            }
            Action::ToggleStartup => {
                autostart::set_enabled(!self.start_with_windows);
                self.start_with_windows = autostart::is_enabled();
                self.render_panel();
                return;
            }
            Action::ToggleOverlay => {
                s.show_overlay = !s.show_overlay;
                if !s.show_overlay {
                    self.overlay.hide();
                }
            }
            Action::StepDown => {
                s.volume_step_db = normalize_step(s.volume_step_db - STEP_INCREMENT_DB)
            }
            Action::StepUp => {
                s.volume_step_db = normalize_step(s.volume_step_db + STEP_INCREMENT_DB)
            }
            Action::ToggleBlock => {
                s.block_scroll = !s.block_scroll;
                self.sync_blocker();
            }
            Action::ModifierPrev => {
                s.modifier = s.modifier.cycle(false);
                self.sync_blocker();
            }
            Action::ModifierNext => {
                s.modifier = s.modifier.cycle(true);
                self.sync_blocker();
            }
            Action::ToggleAddList => {
                self.adding = !self.adding;
                if self.adding {
                    self.apps_loaded = false;
                    self.audio.send(Command::ListApps);
                }
                self.render_panel();
                return;
            }
            Action::RemoveTarget(index) => {
                if index < s.targets.len() {
                    s.targets.remove(index);
                }
            }
            Action::AddTarget(exe) => {
                s.targets.push(exe);
                s.targets = normalize_targets(&s.targets);
            }
            Action::Quit => {
                self.close_panel();
                unsafe { PostQuitMessage(0) };
                return;
            }
        }
        self.store.save(&self.settings);
        self.render_panel();
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.settings.enabled = enabled;
        self.wheel.reset();
        self.blocker.stop();
        self.tray.set_enabled(enabled);
        self.store.save(&self.settings);
    }

    fn open_panel(&mut self) {
        self.start_with_windows = autostart::is_enabled();
        self.adding = false;
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let available = self.available();
        let view = View {
            enabled: self.settings.enabled,
            start_with_windows: self.start_with_windows,
            show_overlay: self.settings.show_overlay,
            block_scroll: self.settings.block_scroll,
            step_db: self.settings.volume_step_db,
            modifier: self.settings.modifier,
            targets: &self.settings.targets,
            adding: self.adding,
            available: &available,
            apps_loaded: self.apps_loaded,
        };
        self.panel.show(&self.gfx, cursor, &view);
    }

    fn close_panel(&mut self) {
        self.panel.hide();
        self.adding = false;
    }

    fn render_panel(&mut self) {
        if self.panel.is_visible() {
            let available = self.available();
            let view = View {
                enabled: self.settings.enabled,
                start_with_windows: self.start_with_windows,
                show_overlay: self.settings.show_overlay,
                block_scroll: self.settings.block_scroll,
                step_db: self.settings.volume_step_db,
                modifier: self.settings.modifier,
                targets: &self.settings.targets,
                adding: self.adding,
                available: &available,
                apps_loaded: self.apps_loaded,
            };
            self.panel.render(&self.gfx, &view);
        }
    }

    /// Playing apps that are not targets yet.
    fn available(&self) -> Vec<String> {
        self.apps
            .iter()
            .filter(|exe| {
                !self
                    .settings
                    .targets
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(exe))
            })
            .cloned()
            .collect()
    }
}

pub fn run() -> windows::core::Result<()> {
    unsafe {
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );
    }

    // The host window is hidden but top-level (not message-only) so it
    // receives the `TaskbarCreated` broadcast after Explorer restarts.
    let host = create_window(w!("Scrollify.Host"), Some(host_proc), WS_EX_TOOLWINDOW)?;
    let panel = create_window(
        w!("Scrollify.Panel"),
        Some(panel_proc),
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
    )?;
    let overlay = create_window(
        w!("Scrollify.Overlay"),
        Some(overlay_proc),
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
    )?;

    let store = ConfigStore::new();
    let settings = store.load();
    let block_scroll = settings.block_scroll;
    let app = App {
        host,
        blocker: Blocker::new(host, WM_HOOK_WHEEL),
        gfx: Graphics::new()?,
        audio: AudioWorker::start(host, WM_AUDIO),
        tray: Tray::new(host, WM_TRAY, settings.enabled),
        panel: Panel::new(panel),
        overlay: Overlay::new(overlay),
        wheel: WheelAccumulator::default(),
        start_with_windows: autostart::is_enabled(),
        adding: false,
        apps: Vec::new(),
        apps_loaded: false,
        store,
        settings,
    };
    APP.with(|slot| *slot.borrow_mut() = Some(app));
    input::register(host)?;
    input::set_keyboard(host, block_scroll);

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    input::unregister();
    // Dropping the app removes the tray icon and stops the audio worker.
    APP.with(|slot| slot.borrow_mut().take());
    unsafe {
        for hwnd in [overlay, panel, host] {
            let _ = DestroyWindow(hwnd);
        }
    }
    Ok(())
}

fn create_window(
    class: PCWSTR,
    proc: WNDPROC,
    ex_style: WINDOW_EX_STYLE,
) -> windows::core::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            ex_style,
            class,
            w!("Scrollify"),
            WINDOW_STYLE(WS_POPUP.0),
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )
    }
}

fn loword(value: isize) -> u32 {
    (value as usize & 0xffff) as u32
}

/// Signed client coordinates from a mouse message.
fn mouse_xy(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xffff) as u16 as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
    (x, y)
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            if let Some(raw) = input::read(lparam) {
                dispatch(Event::Input(raw));
            }
            // Required so Windows can clean up the input buffer.
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TRAY => {
            dispatch(Event::Tray(loword(lparam.0)));
            LRESULT(0)
        }
        WM_AUDIO => {
            dispatch(Event::Audio);
            LRESULT(0)
        }
        WM_HOOK_WHEEL => {
            dispatch(Event::HookWheel(wparam.0 as isize as i32));
            LRESULT(0)
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            dispatch(Event::TaskbarCreated);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut track);
            let (x, y) = mouse_xy(lparam);
            dispatch(Event::PanelMove(x, y));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            dispatch(Event::PanelLeave);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = mouse_xy(lparam);
            dispatch(Event::PanelClick(x, y));
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            dispatch(Event::PanelClose);
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE => {
            dispatch(Event::PanelClose);
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if loword(wparam.0 as isize) == WA_INACTIVE {
                dispatch(Event::PanelClose);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
                SetCursor(cursor);
            }
            LRESULT(1)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == overlay::TIMER_ID => {
            dispatch(Event::OverlayTick);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
