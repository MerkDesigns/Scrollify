use std::ffi::c_void;
use std::mem::size_of_val;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::app_state::AppState;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{
    EventLoopBuilderExtWindows, WindowBuilderExtWindows, WindowExtWindows,
};
use tao::window::{Theme, Window, WindowBuilder};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, PostThreadMessageW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WM_APP, WM_QUIT,
};
use wry::http::Request;
use wry::{WebView, WebViewBuilder};

const PANEL_WIDTH: i32 = 301;
const PANEL_HEIGHT: i32 = 202;
const SCROLLIFY_LOGO: &[u8] = include_bytes!("../Assets/ScrollifyLOGO.png");
pub const WM_REFRESH_TRAY_ICON: u32 = WM_APP + 2;

#[derive(Clone)]
pub struct SettingsUiHandle {
    proxy: EventLoopProxy<UserEvent>,
    tray_click_in_progress: Arc<AtomicBool>,
}

pub struct SettingsUi {
    handle: SettingsUiHandle,
    thread: Option<JoinHandle<()>>,
}

enum UserEvent {
    Toggle { x: i32, y: i32 },
    Ipc(String),
    Shutdown,
}

impl SettingsUi {
    pub fn start(app: Arc<AppState>) -> windows::core::Result<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let main_thread_id = unsafe { GetCurrentThreadId() };
        let tray_click_in_progress = Arc::new(AtomicBool::new(false));
        let ui_tray_click = tray_click_in_progress.clone();
        let thread =
            thread::spawn(move || run_ui_thread(app, main_thread_id, ready_tx, ui_tray_click));

        let proxy = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| windows::core::Error::from_win32())?
            .map_err(|_| windows::core::Error::from_win32())?;

        Ok(Self {
            handle: SettingsUiHandle {
                proxy,
                tray_click_in_progress,
            },
            thread: Some(thread),
        })
    }

    pub fn handle(&self) -> SettingsUiHandle {
        self.handle.clone()
    }
}

impl SettingsUiHandle {
    pub fn begin_tray_right_click(&self) {
        self.tray_click_in_progress.store(true, Ordering::SeqCst);
    }

    pub fn toggle(&self, cursor: POINT) {
        if self
            .proxy
            .send_event(UserEvent::Toggle {
                x: cursor.x,
                y: cursor.y,
            })
            .is_err()
        {
            self.tray_click_in_progress.store(false, Ordering::SeqCst);
        }
    }
}

impl Drop for SettingsUi {
    fn drop(&mut self) {
        let _ = self.handle.proxy.send_event(UserEvent::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_ui_thread(
    app: Arc<AppState>,
    main_thread_id: u32,
    ready_tx: mpsc::Sender<Result<EventLoopProxy<UserEvent>, String>>,
    tray_click_in_progress: Arc<AtomicBool>,
) {
    unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            let _ = ready_tx.send(Err("failed to initialize COM".to_string()));
            return;
        }
    }

    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    event_loop_builder.with_any_thread(true);
    let event_loop = event_loop_builder.build();
    let proxy = event_loop.create_proxy();

    let window = match WindowBuilder::new()
        .with_title("Scrollify")
        .with_inner_size(LogicalSize::new(PANEL_WIDTH as f64, PANEL_HEIGHT as f64))
        .with_resizable(false)
        .with_visible(false)
        .with_decorations(false)
        .with_transparent(false)
        .with_theme(Some(Theme::Dark))
        .with_always_on_top(true)
        .with_skip_taskbar(true)
        .with_undecorated_shadow(false)
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(error) => {
            let _ = ready_tx.send(Err(error.to_string()));
            unsafe {
                CoUninitialize();
            }
            return;
        }
    };

    configure_window_style(&window);

    let ipc_proxy = proxy.clone();
    let builder = WebViewBuilder::new()
        .with_ipc_handler(move |request: Request<String>| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(request.body().clone()));
        })
        .with_accept_first_mouse(true);

    let panel_html = PANEL_HTML.replace("__SCROLLIFY_LOGO__", &BASE64.encode(SCROLLIFY_LOGO));
    let webview = match builder.with_html(panel_html).build(&window) {
        Ok(webview) => webview,
        Err(error) => {
            let _ = ready_tx.send(Err(error.to_string()));
            unsafe {
                CoUninitialize();
            }
            return;
        }
    };

    refresh_webview(&webview, &app);
    let _ = ready_tx.send(Ok(proxy));

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::UserEvent(UserEvent::Toggle { x, y }) => {
                if window.is_visible() {
                    window.set_visible(false);
                } else {
                    refresh_webview(&webview, &app);
                    let position = panel_position(x, y);
                    window.set_outer_position(PhysicalPosition::new(position.0, position.1));
                    window.set_visible(true);
                    window.set_focus();
                }
                tray_click_in_progress.store(false, Ordering::SeqCst);
            }
            Event::UserEvent(UserEvent::Ipc(action)) => {
                if action == "dismiss" {
                    window.set_visible(false);
                } else {
                    handle_action(&app, main_thread_id, &action, control_flow);
                    refresh_webview(&webview, &app);
                    if action == "quit" {
                        window.set_visible(false);
                    }
                }
            }
            Event::UserEvent(UserEvent::Shutdown) => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => {
                if !tray_click_in_progress.load(Ordering::SeqCst) {
                    window.set_visible(false);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                window.set_visible(false);
            }
            Event::LoopDestroyed => unsafe {
                CoUninitialize();
            },
            _ => {}
        }
    });
}

fn configure_window_style(window: &Window) {
    let hwnd = HWND(window.hwnd() as *mut c_void);
    let dark_mode = BOOL(1);
    let corner = DWMWCP_ROUNDSMALL;
    let no_border: u32 = 0xffff_fffe;

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const BOOL as *const c_void,
            size_of_val(&dark_mode) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const c_void,
            size_of_val(&corner) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &no_border as *const u32 as *const c_void,
            size_of_val(&no_border) as u32,
        );
    }
}

fn handle_action(
    app: &AppState,
    main_thread_id: u32,
    action: &str,
    control_flow: &mut ControlFlow,
) {
    let settings = app.settings();
    match action {
        "toggle-enabled" => {
            app.set_enabled(!settings.enabled);
            notify_main_thread(main_thread_id, WM_REFRESH_TRAY_ICON);
        }
        "toggle-startup" => app.set_start_with_windows(!app.start_with_windows()),
        "step-down" => app.set_volume_step_db(settings.volume_step_db - 0.5),
        "step-up" => app.set_volume_step_db(settings.volume_step_db + 0.5),
        "quit" => {
            notify_main_thread(main_thread_id, WM_QUIT);
            *control_flow = ControlFlow::Exit;
        }
        _ => {}
    }
}

fn notify_main_thread(thread_id: u32, message: u32) {
    unsafe {
        let _ = PostThreadMessageW(thread_id, message, WPARAM(0), LPARAM(0));
    }
}

fn panel_position(cursor_x: i32, cursor_y: i32) -> (i32, i32) {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let right = left + width;
        let bottom = top + height;

        let x = (cursor_x - PANEL_WIDTH + 8).clamp(left + 8, right - PANEL_WIDTH - 8);
        let y = (cursor_y - PANEL_HEIGHT - 8).clamp(top + 8, bottom - PANEL_HEIGHT - 8);
        (x, y)
    }
}

fn refresh_webview(webview: &WebView, app: &AppState) {
    let settings = app.settings();
    let state = serde_json::json!({
        "enabled": settings.enabled,
        "startWithWindows": app.start_with_windows(),
        "volumeStepDb": format!("{:.1}", settings.volume_step_db),
    });
    let script = format!("window.Scrollify.setState({state});");
    let _ = webview.evaluate_script(&script);
}

const PANEL_HTML: &str = r#"
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <style>
    * { box-sizing: border-box; }

    html, body {
      width: 100%;
      height: 100%;
      margin: 0;
      overflow: hidden;
      background: #000000;
      font-family: "Segoe UI Variable Text", "Segoe UI", Arial, sans-serif;
      user-select: none;
    }

    body {
      width: 430px;
      height: 288px;
      color: #f7f8f6;
      padding: 0;
      zoom: .7;
    }

    .panel {
      height: 100%;
      background: #080808;
      border: 1.5px solid #343434;
      border-radius: 10px;
      box-shadow: none;
      overflow: hidden;
    }

    .top {
      height: 74px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 0 22px;
    }

    .brand {
      display: flex;
      align-items: center;
      gap: 11px;
      min-width: 0;
    }

    .brand-logo {
      width: 39px;
      height: 39px;
      flex: 0 0 auto;
      border-radius: 10px;
      object-fit: cover;
      filter: drop-shadow(0 0 9px rgba(126, 239, 51, .16));
    }

    .brand-name {
      font-family: "Segoe UI Variable Display", "Segoe UI", Arial, sans-serif;
      font-size: 22px;
      line-height: 1;
      font-weight: 760;
      letter-spacing: .5px;
      color: #f8f9f7;
      text-shadow: 0 2px 8px rgba(0, 0, 0, .36);
    }

    button {
      appearance: none;
      font: inherit;
      outline: none;
      cursor: pointer;
    }

    button:focus-visible {
      box-shadow: 0 0 0 2px rgba(139, 239, 70, .42);
    }

    button:active {
      transform: translateY(1px);
    }

    .setting-row:active {
      transform: none;
    }

    .status {
      min-width: 88px;
      height: 39px;
      padding: 0 15px;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 9px;
      border: 1px solid rgba(118, 130, 122, .48);
      border-radius: 999px;
      color: #aeb4b0;
      background: #151515;
      font-size: 15px;
      font-weight: 700;
      letter-spacing: .4px;
      transition: border-color .16s ease, color .16s ease, background .16s ease;
    }

    .status.on {
      color: #f9fff6;
      border-color: #557b3e;
      background: #182415;
      box-shadow: none;
    }

    .status-dot {
      width: 13px;
      height: 13px;
      border-radius: 50%;
      background: #68706b;
      box-shadow: inset 0 1px 1px rgba(255, 255, 255, .14);
      transition: background .16s ease, box-shadow .16s ease;
    }

    .status.on .status-dot {
      background: #8bed45;
      box-shadow:
        0 0 8px rgba(139, 237, 69, .75),
        0 0 18px rgba(139, 237, 69, .36);
    }

    .divider {
      height: 1px;
      margin: 0 21px;
      background: #242424;
    }

    .content {
      padding: 0 21px 16px;
    }

    .setting-row,
    .volume-row {
      width: 100%;
      height: 61px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 14px;
    }

    .setting-row {
      padding: 0;
      border: 0;
      color: inherit;
      background: transparent;
      text-align: left;
    }

    .setting-row:hover .setting-label,
    .volume-row:hover .setting-label {
      color: #ffffff;
    }

    .setting-title {
      min-width: 0;
      display: flex;
      align-items: center;
      gap: 13px;
    }

    .setting-icon {
      width: 32px;
      height: 32px;
      flex: 0 0 auto;
      display: grid;
      place-items: center;
      border-radius: 50%;
      color: #8beb49;
    }

    .setting-icon.power-icon {
      background: #21301d;
      box-shadow: none;
    }

    .setting-icon svg {
      width: 22px;
      height: 22px;
      overflow: visible;
      filter: drop-shadow(0 0 5px rgba(139, 235, 73, .22));
    }

    .setting-label {
      overflow: hidden;
      color: #eef1ee;
      font-size: 19.2px;
      line-height: 1;
      font-weight: 520;
      white-space: nowrap;
      transition: color .14s ease;
    }

    .switch {
      width: 59px;
      height: 32px;
      flex: 0 0 auto;
      position: relative;
      display: block;
      border: 1px solid rgba(104, 115, 108, .55);
      border-radius: 999px;
      background: #151515;
      box-shadow: inset 0 2px 5px rgba(0, 0, 0, .34);
      transition: border-color .16s ease, background .16s ease;
    }

    .knob {
      position: absolute;
      top: 3px;
      left: 3px;
      width: 24px;
      height: 24px;
      border-radius: 50%;
      background: linear-gradient(145deg, #c5cac6, #919994);
      box-shadow: 0 2px 5px rgba(0, 0, 0, .42);
      transition: transform .17s ease, background .17s ease, box-shadow .17s ease;
    }

    .setting-row.on .switch {
      border-color: #59813f;
      background: #344b29;
      box-shadow: inset 0 2px 5px rgba(0, 0, 0, .25);
    }

    .setting-row.on .knob {
      background: linear-gradient(145deg, #ffffff, #dcebd7);
      box-shadow:
        0 2px 5px rgba(0, 0, 0, .38),
        0 0 9px rgba(194, 255, 160, .22);
      transform: translateX(27px);
    }

    .row-divider {
      margin: 0;
    }

    .stepper {
      flex: 0 0 auto;
      display: flex;
      align-items: center;
      gap: 6px;
    }

    .stepper button {
      width: 40px;
      height: 40px;
      display: grid;
      place-items: center;
      border: 1px solid rgba(96, 105, 99, .5);
      border-radius: 9px;
      color: #d9ddda;
      background: #151515;
      font-size: 25px;
      font-weight: 300;
      line-height: 1;
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .025);
      transition: border-color .14s ease, color .14s ease, background .14s ease;
    }

    .stepper button:hover {
      border-color: rgba(139, 235, 73, .48);
      color: #92ef52;
      background: rgba(39, 48, 41, .96);
    }

    .step-value {
      width: 81px;
      height: 40px;
      display: flex;
      align-items: center;
      justify-content: center;
      border: 1px solid rgba(96, 105, 99, .5);
      border-radius: 9px;
      background: #151515;
      color: #f7f8f6;
      font-size: 15px;
      font-weight: 650;
      white-space: nowrap;
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .025);
    }

    .quit {
      width: 100%;
      height: 47px;
      position: relative;
      margin-top: 9px;
      border: 1px solid rgba(119, 129, 123, .42);
      border-radius: 10px;
      color: #f3f5f2;
      background: #151515;
      font-size: 16px;
      font-weight: 650;
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .025);
      transition: border-color .14s ease, background .14s ease;
    }

    .quit:hover {
      border-color: rgba(139, 235, 73, .42);
      background:
        linear-gradient(90deg, rgba(126, 221, 66, .035), rgba(255, 255, 255, .015)),
        rgba(31, 37, 33, .95);
    }

    .quit-icon {
      width: 22px;
      height: 22px;
      position: absolute;
      top: 12px;
      right: 16px;
      color: #858d88;
      transition: color .14s ease, transform .14s ease;
    }

    .quit:hover .quit-icon {
      color: #a7b0aa;
      transform: translateX(2px);
    }
  </style>
</head>
<body>
  <main class="panel">
    <header class="top">
      <div class="brand">
        <img class="brand-logo" src="data:image/png;base64,__SCROLLIFY_LOGO__" alt="">
        <div class="brand-name">SCROLLIFY</div>
      </div>
      <button id="power" class="status" type="button" aria-label="Toggle Scrollify">
        <span class="status-dot"></span>
        <span id="power-label">OFF</span>
      </button>
    </header>

    <div class="divider"></div>

    <section class="content">
      <button id="startup" class="setting-row" type="button">
        <span class="setting-title">
          <span class="setting-icon power-icon">
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d="M12 2.7v8.1M6.1 5.9a8 8 0 1 0 11.8 0" stroke="currentColor" stroke-width="2.3" stroke-linecap="round"/>
            </svg>
          </span>
          <span class="setting-label">Start with Windows</span>
        </span>
        <span class="switch" aria-hidden="true"><span class="knob"></span></span>
      </button>

      <div class="divider row-divider"></div>

      <div class="volume-row">
        <div class="setting-title">
          <span class="setting-icon">
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d="M4 9.2v5.6h4l5 4V5l-5 4.2H4Z" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>
              <path d="M16 8.5a5 5 0 0 1 0 7M18.5 6a8.5 8.5 0 0 1 0 12" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
            </svg>
          </span>
          <span class="setting-label">Volume Step</span>
        </div>
        <div class="stepper">
          <button id="down" type="button" aria-label="Decrease volume step">&minus;</button>
          <div id="step" class="step-value">1.0 dB</div>
          <button id="up" type="button" aria-label="Increase volume step">+</button>
        </div>
      </div>

      <button id="quit" class="quit" type="button">
        Quit
        <svg class="quit-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path d="M10 5H5.5A1.5 1.5 0 0 0 4 6.5v11A1.5 1.5 0 0 0 5.5 19H10M14 8l4 4-4 4M9 12h9" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
    </section>
  </main>

  <script>
    const send = action => window.ipc.postMessage(action);
    const els = {
      power: document.getElementById('power'),
      powerLabel: document.getElementById('power-label'),
      startup: document.getElementById('startup'),
      step: document.getElementById('step')
    };

    document.getElementById('power').onclick = () => send('toggle-enabled');
    document.getElementById('startup').onclick = () => send('toggle-startup');
    document.getElementById('down').onclick = () => send('step-down');
    document.getElementById('up').onclick = () => send('step-up');
    document.getElementById('quit').onclick = () => send('quit');
    document.addEventListener('contextmenu', event => {
      event.preventDefault();
      send('dismiss');
    });

    window.Scrollify = {
      setState(state) {
        els.powerLabel.textContent = state.enabled ? 'ON' : 'OFF';
        els.power.classList.toggle('on', state.enabled);
        els.power.setAttribute('aria-pressed', String(state.enabled));
        els.startup.classList.toggle('on', state.startWithWindows);
        els.startup.setAttribute('aria-pressed', String(state.startWithWindows));
        els.step.textContent = `${state.volumeStepDb} dB`;
      }
    };
  </script>
</body>
</html>
"#;
