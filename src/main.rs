//! Screen-wide laser pointer, driven by user-configurable shortcuts (Alt+L toggles it by default).
//! While on: left-drag draws; right clicks and the scroll wheel pass through to the app underneath.
//! A tray icon opens the settings panel.
#![windows_subsystem = "windows"]

mod panel;
mod settings;
mod stroke;
mod ui;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::Instant;

use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::{
    GetLastError, COLORREF, E_FAIL, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT,
    RECT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1DeviceContext5, ID2D1Factory1,
    ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_MAP_OPTIONS_READ, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, IDXGISwapChain2,
    DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetMonitorInfoW, MonitorFromPoint, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, ReleaseCapture, SendInput, SetCapture, UnregisterHotKey, HOT_KEY_MODIFIERS,
    INPUT, INPUT_0, INPUT_MOUSE, MOD_NOREPEAT, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, CreateWindowExW, DefWindowProcW, DestroyCursor, DispatchMessageW,
    GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadCursorW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, RegisterClassW, SetCursor,
    SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, GWL_EXSTYLE, HCURSOR, HICON, HWND_TOPMOST, ICONINFO, IDC_CROSS, LWA_ALPHA,
    MA_NOACTIVATE, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_HOTKEY,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_numerics::Vector2;

use settings::{Binding, Command, Slot, COMMANDS, MAX_PRESETS};
use stroke::{rgba, union, Bounds, Point, Scratch, MIN_STEP};

// Hotkey ids: commands are 1..=8, presets start here.
const HOTKEY_PRESET: i32 = 100;
const HOTKEY_PROBE: i32 = 99;
const TIMER_PASSTHROUGH: usize = 1;
const PASSTHROUGH_MS: u32 = 80;
// wParam flag on WM_MOUSEWHEEL.
const MK_CONTROL: usize = 0x8;

const RIPPLE_SECS: f32 = 0.45;
const TOAST_SECS: f32 = 1.4;

// Excalidraw's light-mode laser cursor (MIT).
const PEN_PATH: &str = "m7.868 11.113 7.773 7.774a2.359 2.359 0 0 0 1.667.691 2.368 2.368 0 0 0 2.357-2.358c0-.625-.248-1.225-.69-1.667L11.201 7.78 9.558 9.469l-1.69 1.643v.001Zm10.273 3.606-3.333 3.333m-3.25-6.583 2 2m-7-7 3 3M3.664 3.625l1 1M2.529 6.922l1.407-.144m5.735-2.932-1.118.866M4.285 9.823l.758-1.194m1.863-6.207-.13 1.408";
const CURSOR_SIZE: f32 = 28.0;
const DOT_CURSOR_SIZE: f32 = 34.0;

static LASER_ON: AtomicBool = AtomicBool::new(false);
/// Kept strokes are on screen, so the clear shortcut has something to do.
static INK_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Bit per command (low bits) and per preset (from bit 16) whose shortcut another app holds.
static HOTKEY_FAILED: AtomicU32 = AtomicU32::new(0);
static CURSOR: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct Ripple {
    x: f32,
    y: f32,
    at: Instant,
}

struct Toast {
    layout: IDWriteTextLayout,
    width: f32,
    height: f32,
    centre: f32,
    bottom: f32,
    at: Instant,
}

struct Graphics {
    swap_chain: IDXGISwapChain1,
    waitable: HANDLE,
    factory: ID2D1Factory1,
    dc: ID2D1DeviceContext,
    target: Option<ID2D1Bitmap1>,
    brush: ID2D1SolidColorBrush,
    dwrite: IDWriteFactory,
    toast_text: IDWriteTextFormat,
    size: (u32, u32),
    // Kept alive for the lifetime of the window; dropping them detaches the visual tree.
    _composition: (
        IDCompositionDevice,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
}

struct App {
    hwnd: HWND,
    gfx: Graphics,
    strokes: Vec<Vec<Point>>,
    drawing: bool,
    dirty: bool,
    scale: f32,
    /// Virtual-screen position of the window, to map screen coordinates into it.
    origin: (i32, i32),
    ripples: Vec<Ripple>,
    toast: Option<Toast>,
    shown: bool,
    /// What each swap-chain buffer last had drawn on it, so a frame only clears and
    /// presents the parts of the screen that changed.
    buffers: [Option<Bounds>; 2],
    back: usize,
    presented: bool,
    scratch: Scratch,
    cursor_key: Option<(u32, bool)>,
    cursor_owned: bool,
}

fn main() -> Result<()> {
    unsafe {
        let _single_instance = CreateMutexW(None, false, w!("Local\\LaserPointer"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            ui::show_running_instance();
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        settings::load();
        let instance = GetModuleHandleW(None)?;
        let class = w!("LaserPointerOverlay");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        });

        let (x, y, width, height) = virtual_screen();
        // Layered only so WS_EX_TRANSPARENT can make it click-through.
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
            class,
            w!("Laser pointer"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
        OVERLAY.store(hwnd.0 as isize, Ordering::Relaxed);

        let scale = GetDpiForWindow(hwnd) as f32 / 96.0;
        let gfx = Graphics::new(hwnd, width as u32, height as u32, scale)?;

        // Stays hidden until there's something to show, so nothing sits over the desktop.
        APP.with(|app| {
            *app.borrow_mut() = Some(App {
                hwnd,
                gfx,
                strokes: Vec::new(),
                drawing: false,
                dirty: false,
                scale,
                origin: (x, y),
                ripples: Vec::new(),
                toast: None,
                shown: false,
                buffers: [Some(full_bounds(width as u32, height as u32)); 2],
                back: 0,
                presented: false,
                scratch: Scratch::default(),
                cursor_key: None,
                cursor_owned: false,
            })
        });
        with_app(App::refresh_cursor);
        // Shortcuts first: adding the tray icon can take Explorer a few seconds.
        sync_hotkeys();
        ui::init(instance.into())?;
        run();
        ui::shutdown();
    }
    Ok(())
}

/// Sleeps in the kernel when nothing is moving, and paces frames off the swap chain's latency
/// waitable while something is, so input is read as late as possible before each frame.
unsafe fn run() {
    let mut msg = MSG::default();
    loop {
        let (animating, waitable) = APP.with(|app| {
            let app = app.borrow();
            let app = app.as_ref().expect("app initialised before the loop");
            (app.needs_frame(), app.gfx.waitable)
        });
        let handles: &[HANDLE] = if animating {
            std::slice::from_ref(&waitable)
        } else {
            &[]
        };
        let woke =
            MsgWaitForMultipleObjectsEx(Some(handles), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if animating && woke == WAIT_OBJECT_0 {
            with_app(App::render);
        }
    }
}

/// try_borrow, because APIs called from a handler can send this window messages synchronously.
fn with_app<R>(action: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|app| {
        let mut app = app.try_borrow_mut().ok()?;
        app.as_mut().map(action)
    })
}

fn overlay() -> HWND {
    HWND(OVERLAY.load(Ordering::Relaxed) as _)
}

pub fn laser_on() -> bool {
    LASER_ON.load(Ordering::Relaxed)
}

pub fn strokes_on_screen() -> bool {
    INK_VISIBLE.load(Ordering::Relaxed)
}

pub fn set_laser(on: bool) {
    with_app(|app| app.set_on(on));
    sync_hotkeys();
    ui::laser_changed();
}

pub fn quit() {
    unsafe { PostQuitMessage(0) };
}

/// After the panel or tray menu changes a setting.
pub fn settings_changed() {
    with_app(|app| {
        app.refresh_cursor();
        app.dirty = true;
    });
    ui::settings_changed();
}

pub fn run_command(command: Command) {
    match command {
        Command::Toggle => set_laser(!laser_on()),
        Command::Off => set_laser(false),
        Command::Undo => {
            with_app(App::undo);
        }
        Command::Clear => {
            with_app(App::clear);
        }
        Command::ToggleInk => {
            let mut current = settings::get();
            current.look.ink = !current.look.ink;
            settings::set(current);
            settings::save();
            let text = if current.look.ink {
                "Keep strokes on"
            } else {
                "Keep strokes off"
            };
            with_app(|app| {
                app.notify(text);
                app.update_ink_visible();
            });
            settings_changed();
        }
        Command::NextPreset | Command::PreviousPreset => {
            let count = settings::presets().len();
            if count > 0 {
                let step = if command == Command::NextPreset { 1 } else { count - 1 };
                let next = settings::active_preset().map_or(0, |index| (index + step) % count);
                apply_preset(next);
            }
        }
        Command::Quit => quit(),
    }
    sync_hotkeys();
}

pub fn apply_preset(index: usize) {
    if let Some(name) = settings::apply_preset(index) {
        with_app(|app| app.notify(&name));
        settings_changed();
    }
}

unsafe fn register(id: i32, binding: Binding) -> bool {
    RegisterHotKey(
        Some(overlay()),
        id,
        HOT_KEY_MODIFIERS(binding.mods) | MOD_NOREPEAT,
        binding.vk,
    )
    .is_ok()
}

/// Claims every shortcut that means something right now. Bare keys like Esc only while the
/// laser is on, and nothing at all while the settings panel has the keyboard.
pub fn sync_hotkeys() {
    let hwnd = Some(overlay());
    let settings = settings::get();
    let presets = settings::presets();
    unsafe {
        for id in 1..=COMMANDS.len() as i32 {
            let _ = UnregisterHotKey(hwnd, id);
        }
        for index in 0..MAX_PRESETS as i32 {
            let _ = UnregisterHotKey(hwnd, HOTKEY_PRESET + index);
        }
        if ui::panel_open() {
            return;
        }
        let on = laser_on();
        let ink = INK_VISIBLE.load(Ordering::Relaxed);
        let mut failed = 0;
        for command in COMMANDS {
            let active = match command {
                Command::Off | Command::Undo => on,
                Command::Clear => on || ink,
                _ => true,
            };
            let key = settings.key(command);
            if active && key.is_set() && !register(command.index() as i32 + 1, key) {
                failed |= 1 << command.index();
            }
        }
        for (index, preset) in presets.iter().enumerate() {
            if preset.key.is_set() && !register(HOTKEY_PRESET + index as i32, preset.key) {
                failed |= 1 << (16 + index);
            }
        }
        HOTKEY_FAILED.store(failed, Ordering::Relaxed);
    }
}

/// Whether another app held this shortcut the last time it was claimed.
pub fn hotkey_failed(slot: Slot) -> bool {
    let bit = match slot {
        Slot::Command(command) => command.index(),
        Slot::Preset(index) => 16 + index,
    };
    HOTKEY_FAILED.load(Ordering::Relaxed) & (1 << bit) != 0
}

/// Whether another app already holds this combination. Only meaningful while the panel is
/// open, when this app has let go of all of its own.
pub fn shortcut_free(binding: Binding) -> bool {
    unsafe {
        let free = register(HOTKEY_PROBE, binding);
        if free {
            let _ = UnregisterHotKey(Some(overlay()), HOTKEY_PROBE);
        }
        free
    }
}

pub fn tray_icon(on: bool, size: u32) -> Option<HICON> {
    let svg = tray_svg(on, settings::get().look.color);
    with_app(|app| unsafe { svg_icon(&app.gfx, &svg, size, None) })?.ok()
}

fn cursor_svg() -> String {
    format!(
        r##"<svg viewBox="0 0 24 24" stroke-width="1" xmlns="http://www.w3.org/2000/svg"><path stroke="#1b1b1f" fill="#fff" d="{PEN_PATH}"/></svg>"##
    )
}

/// The cursor pen, heavier so it reads at tray size, with a dot of the laser colour while on.
fn tray_svg(on: bool, color: u32) -> String {
    let dot = if on {
        format!(
            r##"<circle cx="5.5" cy="5.5" r="5" fill="#{color:06x}" stroke="#1b1b1f" stroke-width="1"/>"##
        )
    } else {
        String::new()
    };
    format!(
        r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path stroke="#1b1b1f" stroke-width="1.6" fill="#fff" d="{PEN_PATH}"/>{dot}</svg>"##
    )
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_HOTKEY => {
                let id = wparam.0 as i32;
                if let Some(&command) = COMMANDS.get((id - 1) as usize) {
                    run_command(command);
                } else if id >= HOTKEY_PRESET {
                    apply_preset((id - HOTKEY_PRESET) as usize);
                }
                LRESULT(0)
            }
            WM_SETCURSOR if laser_on() => {
                SetCursor(Some(HCURSOR(CURSOR.load(Ordering::Relaxed) as _)));
                LRESULT(1)
            }
            // Keeps keyboard focus on the app being presented.
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                with_app(|app| app.begin(client_point(lparam)));
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                with_app(|app| app.extend(client_point(lparam)));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|app| app.drawing = false);
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_RBUTTONDOWN => LRESULT(0),
            WM_RBUTTONUP => {
                with_app(|app| app.ripple(client_point(lparam)));
                pass_through(
                    hwnd,
                    &[
                        mouse_input(MOUSEEVENTF_LEFTDOWN, 0),
                        mouse_input(MOUSEEVENTF_LEFTUP, 0),
                    ],
                );
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
                if wparam.0 & MK_CONTROL != 0 {
                    with_app(|app| app.resize_by(delta));
                    ui::settings_changed();
                } else {
                    pass_through(hwnd, &[mouse_input(MOUSEEVENTF_WHEEL, delta as i32 as u32)]);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_PASSTHROUGH => {
                let _ = KillTimer(Some(hwnd), TIMER_PASSTHROUGH);
                if laser_on() {
                    set_click_through(hwnd, false);
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                with_app(App::fit_to_screen);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn client_point(lparam: LPARAM) -> (f32, f32) {
    // Signed: monitors left of or above the primary give negative coordinates.
    (
        (lparam.0 & 0xffff) as i16 as f32,
        ((lparam.0 >> 16) & 0xffff) as i16 as f32,
    )
}

unsafe fn virtual_screen() -> (i32, i32, i32, i32) {
    (
        GetSystemMetrics(SM_XVIRTUALSCREEN),
        GetSystemMetrics(SM_YVIRTUALSCREEN),
        GetSystemMetrics(SM_CXVIRTUALSCREEN),
        GetSystemMetrics(SM_CYVIRTUALSCREEN),
    )
}

fn full_bounds(width: u32, height: u32) -> Bounds {
    Bounds {
        left: 0.0,
        top: 0.0,
        right: width as f32,
        bottom: height as f32,
    }
}

unsafe fn set_click_through(hwnd: HWND, on: bool) {
    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let flag = WS_EX_TRANSPARENT.0 as isize;
    SetWindowLongPtrW(
        hwnd,
        GWL_EXSTYLE,
        if on { style | flag } else { style & !flag },
    );
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                mouseData: data,
                ..Default::default()
            },
        },
    }
}

/// SendInput only queues the input, so the overlay stays click-through for a moment
/// until the system has routed it to the window underneath.
unsafe fn pass_through(hwnd: HWND, inputs: &[INPUT]) {
    set_click_through(hwnd, true);
    SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    SetTimer(Some(hwnd), TIMER_PASSTHROUGH, PASSTHROUGH_MS, None);
}

impl App {
    fn needs_frame(&self) -> bool {
        if !self.shown {
            return false;
        }
        // Kept strokes don't change on their own, so only repaint them when something new arrives.
        let fading = !settings::get().look.ink && self.has_strokes();
        self.dirty || fading || !self.ripples.is_empty() || self.toast.is_some()
    }

    fn has_strokes(&self) -> bool {
        self.strokes.iter().any(|points| !points.is_empty())
    }

    /// Anything left to show once the laser is off: kept strokes, a fading tail, a notice.
    fn has_content(&self) -> bool {
        self.has_strokes() || !self.ripples.is_empty() || self.toast.is_some()
    }

    fn update_ink_visible(&self) {
        let visible = settings::get().look.ink && self.has_strokes();
        INK_VISIBLE.store(visible, Ordering::Relaxed);
    }

    fn set_on(&mut self, on: bool) {
        LASER_ON.store(on, Ordering::Relaxed);
        // Kept strokes outlive the laser; they stay on screen until cleared.
        if !settings::get().look.ink {
            self.strokes.clear();
        }
        self.ripples.clear();
        self.drawing = false;
        self.dirty = true;
        self.toast = None;
        self.notify(if on { "Laser on" } else { "Laser off" });
        unsafe {
            set_click_through(self.hwnd, !on);
            if on {
                self.show();
                // Another always-on-top window (a slideshow, say) may have been raised since.
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                SetCursor(Some(HCURSOR(CURSOR.load(Ordering::Relaxed) as _)));
            } else {
                let _ = ReleaseCapture();
                if !self.has_content() {
                    self.hide();
                }
            }
        }
        self.update_ink_visible();
    }

    /// Paints before showing, so an old frame never flashes up.
    fn show(&mut self) {
        if self.shown {
            return;
        }
        self.render();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.shown = true;
    }

    fn hide(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.shown = false;
    }

    /// A pill near the bottom of the screen, if the user wants them.
    fn notify(&mut self, text: &str) {
        if !settings::get().indicator {
            return;
        }
        self.toast = self.make_toast(text);
        self.dirty = true;
        self.show();
    }

    fn make_toast(&self, text: &str) -> Option<Toast> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self
                .gfx
                .dwrite
                .CreateTextLayout(&wide, &self.gfx.toast_text, 4000.0, 400.0)
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info);
            let work = info.rcWork;
            Some(Toast {
                layout,
                width: metrics.width,
                height: metrics.height,
                centre: ((work.left + work.right) / 2 - self.origin.0) as f32,
                bottom: (work.bottom - self.origin.1) as f32 - 72.0 * self.scale,
                at: Instant::now(),
            })
        }
    }

    fn begin(&mut self, (x, y): (f32, f32)) {
        if !laser_on() {
            return;
        }
        self.drawing = true;
        self.dirty = true;
        self.strokes.push(vec![Point {
            x,
            y,
            at: Instant::now(),
        }]);
        self.update_ink_visible();
    }

    fn extend(&mut self, (x, y): (f32, f32)) {
        if !self.drawing {
            return;
        }
        let Some(points) = self.strokes.last_mut() else {
            return;
        };
        if let Some(last) = points.last() {
            if (x - last.x).hypot(y - last.y) < MIN_STEP * self.scale {
                return;
            }
        }
        points.push(Point {
            x,
            y,
            at: Instant::now(),
        });
        self.dirty = true;
    }

    fn undo(&mut self) {
        if self.drawing {
            return;
        }
        self.strokes.retain(|points| !points.is_empty());
        if self.strokes.pop().is_some() {
            self.dirty = true;
        }
        self.update_ink_visible();
    }

    fn clear(&mut self) {
        self.strokes.clear();
        self.drawing = false;
        self.dirty = true;
        self.update_ink_visible();
    }

    fn ripple(&mut self, (x, y): (f32, f32)) {
        if settings::get().ripples {
            self.ripples.push(Ripple {
                x,
                y,
                at: Instant::now(),
            });
        }
    }

    fn resize_by(&mut self, delta: i16) {
        let mut settings = settings::get();
        let width = settings.look.width as i32 + i32::from(delta.signum());
        settings.look.width = width.clamp(2, 24) as u32;
        settings::set(settings);
        settings::save();
        self.notify(&format!("Thickness  {} px", settings.look.width));
        self.dirty = true;
    }

    fn fit_to_screen(&mut self) {
        let (x, y, width, height) = unsafe { virtual_screen() };
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
            let _ = self.gfx.resize(width as u32, height as u32);
        }
        self.origin = (x, y);
        self.strokes.clear();
        self.buffers = [Some(full_bounds(width as u32, height as u32)); 2];
        self.back = 0;
        self.presented = false;
        self.dirty = true;
        self.update_ink_visible();
    }

    /// Rebuilds the cursor when its colour or kind changes.
    fn refresh_cursor(&mut self) {
        let settings = settings::get();
        let key = (settings.look.color, settings.dot_cursor);
        if self.cursor_key == Some(key) {
            return;
        }
        self.cursor_key = Some(key);
        let made = unsafe {
            if settings.dot_cursor {
                dot_cursor(&self.gfx, settings.look.color, self.scale)
            } else {
                let size = (CURSOR_SIZE * self.scale).round() as u32;
                svg_icon(&self.gfx, &cursor_svg(), size, Some((0, 0)))
            }
        };
        let cursor = match &made {
            Ok(icon) => HCURSOR(icon.0),
            Err(_) => unsafe { LoadCursorW(None, IDC_CROSS).unwrap_or_default() },
        };
        let old = HCURSOR(CURSOR.swap(cursor.0 as isize, Ordering::Relaxed) as _);
        unsafe {
            if laser_on() {
                SetCursor(Some(cursor));
            }
            if self.cursor_owned && !old.is_invalid() {
                let _ = DestroyCursor(old);
            }
        }
        self.cursor_owned = made.is_ok();
    }

    fn render(&mut self) {
        let settings = settings::get();
        let now = Instant::now();
        self.ripples
            .retain(|ripple| now.duration_since(ripple.at).as_secs_f32() < RIPPLE_SECS);
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| now.duration_since(toast.at).as_secs_f32() >= TOAST_SECS)
        {
            self.toast = None;
        }

        let Self {
            gfx,
            strokes,
            scratch,
            ripples,
            toast,
            scale,
            buffers,
            back,
            drawing,
            ..
        } = self;
        let scale = *scale;
        let target: &ID2D1RenderTarget = &gfx.dc;
        let mut drawn = None;
        unsafe {
            gfx.dc.BeginDraw();
            // Everything outside what this buffer last showed is already transparent.
            if let Some(stale) = buffers[*back] {
                gfx.dc.PushAxisAlignedClip(&rect_f(stale), D2D1_ANTIALIAS_MODE_ALIASED);
                gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
                gfx.dc.PopAxisAlignedClip();
            }
            for points in strokes.iter_mut() {
                let bounds = stroke::draw(
                    &gfx.factory,
                    target,
                    &gfx.brush,
                    points,
                    now,
                    &settings.look,
                    scale,
                    scratch,
                );
                drawn = union(drawn, bounds);
            }
            // Faded-out strokes go, except the one still being drawn.
            let count = strokes.len();
            let mut index = 0;
            strokes.retain(|points| {
                index += 1;
                !points.is_empty() || (*drawing && index == count)
            });
            for ripple in ripples.iter() {
                let k = now.duration_since(ripple.at).as_secs_f32() / RIPPLE_SECS;
                let radius = (6.0 + 30.0 * (1.0 - (1.0 - k).powi(3))) * scale;
                gfx.brush
                    .SetColor(&rgba(settings.look.color, 0.9 * (1.0 - k).powf(1.5)));
                gfx.dc.DrawEllipse(
                    &ellipse(ripple.x, ripple.y, radius),
                    &gfx.brush,
                    3.0 * scale * (1.0 - 0.5 * k),
                    None,
                );
                drawn = union(drawn, Some(Bounds::around(ripple.x, ripple.y, radius + 4.0 * scale)));
            }
            if let Some(toast) = toast {
                let age = now.duration_since(toast.at).as_secs_f32();
                let alpha = (age / 0.15).min(1.0).min((TOAST_SECS - age) / 0.3).max(0.0);
                drawn = union(
                    drawn,
                    Some(gfx.draw_toast(toast, settings.look.color, alpha, scale)),
                );
            }
            let _ = gfx.dc.EndDraw(None, None);
        }

        // Present only what changed since the frame now on screen.
        let changed = union(drawn, buffers[1 - *back]);
        let dirty = changed.map_or(
            RECT {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            },
            |bounds| pixel_rect(bounds, gfx.size, 2.0 * scale),
        );
        let mut rects = [dirty];
        let parameters = DXGI_PRESENT_PARAMETERS {
            // The first frame after (re)creating the buffers must be presented whole.
            DirtyRectsCount: u32::from(self.presented),
            pDirtyRects: rects.as_mut_ptr(),
            ..Default::default()
        };
        unsafe {
            let _ = gfx.swap_chain.Present1(1, DXGI_PRESENT(0), &parameters);
        }
        buffers[*back] = drawn.map(|bounds| inflate(bounds, 2.0 * scale));
        *back ^= 1;
        self.presented = true;
        self.dirty = false;
        let was_visible = INK_VISIBLE.load(Ordering::Relaxed);
        self.update_ink_visible();
        if was_visible != INK_VISIBLE.load(Ordering::Relaxed) {
            sync_hotkeys();
        }
        if !laser_on() && !self.has_content() && self.shown {
            self.hide();
        }
    }
}

fn inflate(bounds: Bounds, by: f32) -> Bounds {
    Bounds {
        left: bounds.left - by,
        top: bounds.top - by,
        right: bounds.right + by,
        bottom: bounds.bottom + by,
    }
}

fn rect_f(bounds: Bounds) -> D2D_RECT_F {
    D2D_RECT_F {
        left: bounds.left.floor(),
        top: bounds.top.floor(),
        right: bounds.right.ceil(),
        bottom: bounds.bottom.ceil(),
    }
}

fn pixel_rect(bounds: Bounds, (width, height): (u32, u32), margin: f32) -> RECT {
    let clamp = |value: f32, max: u32| (value.max(0.0) as i32).min(max as i32);
    let rect = RECT {
        left: clamp((bounds.left - margin).floor(), width),
        top: clamp((bounds.top - margin).floor(), height),
        right: clamp((bounds.right + margin).ceil(), width),
        bottom: clamp((bounds.bottom + margin).ceil(), height),
    };
    if rect.right <= rect.left || rect.bottom <= rect.top {
        RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        }
    } else {
        rect
    }
}

fn ellipse(x: f32, y: f32, radius: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2 { X: x, Y: y },
        radiusX: radius,
        radiusY: radius,
    }
}

fn bitmap_properties(options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: options,
        ..Default::default()
    }
}

impl Graphics {
    unsafe fn new(hwnd: HWND, width: u32, height: u32, scale: f32) -> Result<Self> {
        let mut d3d: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        )?;
        let dxgi: IDXGIDevice = d3d
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?
            .cast()?;

        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let swap_chain = factory.CreateSwapChainForComposition(
            &dxgi,
            &DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
                ..Default::default()
            },
            None,
        )?;
        let latency: IDXGISwapChain2 = swap_chain.cast()?;
        latency.SetMaximumFrameLatency(1)?;
        let waitable = latency.GetFrameLatencyWaitableObject();

        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = d2d
            .CreateDevice(&dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let brush = dc.CreateSolidColorBrush(&rgba(settings::get().look.color, 1.0), None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let toast_text = dwrite.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            15.0 * scale,
            w!("en-us"),
        )?;

        let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = composition.CreateTargetForHwnd(hwnd, true)?;
        let visual = composition.CreateVisual()?;
        visual.SetContent(&swap_chain)?;
        target.SetRoot(&visual)?;
        composition.Commit()?;

        let mut gfx = Self {
            swap_chain,
            waitable,
            factory: d2d,
            dc,
            target: None,
            brush,
            dwrite,
            toast_text,
            size: (width, height),
            _composition: (composition, target, visual),
        };
        gfx.bind_target()?;
        Ok(gfx)
    }

    unsafe fn bind_target(&mut self) -> Result<()> {
        let surface: IDXGISurface = self.swap_chain.GetBuffer(0)?;
        let target = self.dc.CreateBitmapFromDxgiSurface(
            &surface,
            Some(&bitmap_properties(
                D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            )),
        )?;
        self.dc.SetTarget(&target);
        self.target = Some(target);
        Ok(())
    }

    unsafe fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.dc.SetTarget(None);
        self.target = None;
        self.swap_chain.ResizeBuffers(
            2,
            width,
            height,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0),
        )?;
        self.size = (width, height);
        self.bind_target()
    }

    unsafe fn draw_toast(&self, toast: &Toast, color: u32, alpha: f32, scale: f32) -> Bounds {
        let (pad, dot, gap) = (18.0 * scale, 5.0 * scale, 10.0 * scale);
        let width = toast.width + pad * 2.0 + dot * 2.0 + gap;
        let height = toast.height + 18.0 * scale;
        let left = toast.centre - width / 2.0;
        let top = toast.bottom - height;
        let rect = D2D_RECT_F {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        self.brush.SetColor(&rgba(0x1c1c22, 0.9 * alpha));
        self.dc.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect,
                radiusX: height / 2.0,
                radiusY: height / 2.0,
            },
            &self.brush,
        );
        self.brush.SetColor(&rgba(color, alpha));
        self.dc
            .FillEllipse(&ellipse(left + pad + dot, top + height / 2.0, dot), &self.brush);
        self.brush.SetColor(&rgba(0xffffff, alpha));
        self.dc.DrawTextLayout(
            Vector2 {
                X: left + pad + dot * 2.0 + gap,
                Y: top + (height - toast.height) / 2.0,
            },
            &toast.layout,
            &self.brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        Bounds {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }
}

/// A glowing dot in the laser colour, like the real thing.
unsafe fn dot_cursor(gfx: &Graphics, color: u32, scale: f32) -> Result<HICON> {
    let size = (DOT_CURSOR_SIZE * scale).round() as u32;
    let centre = size as f32 / 2.0;
    render_icon(gfx, size, Some((size / 2, size / 2)), |dc| {
        let stops = ID2D1RenderTarget::CreateGradientStopCollection(
            dc,
            &[
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: rgba(color, 0.65),
                },
                D2D1_GRADIENT_STOP {
                    position: 0.35,
                    color: rgba(color, 0.35),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: rgba(color, 0.0),
                },
            ],
            D2D1_GAMMA_2_2,
            D2D1_EXTEND_MODE_CLAMP,
        )?;
        let glow = dc.CreateRadialGradientBrush(
            &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 {
                    X: centre,
                    Y: centre,
                },
                gradientOriginOffset: Vector2::default(),
                radiusX: centre,
                radiusY: centre,
            },
            None,
            &stops,
        )?;
        dc.FillEllipse(&ellipse(centre, centre, centre), &glow);
        let solid = dc.CreateSolidColorBrush(&rgba(color, 1.0), None)?;
        dc.FillEllipse(&ellipse(centre, centre, 5.0 * scale), &solid);
        solid.SetColor(&rgba(0xffffff, 0.9));
        dc.FillEllipse(&ellipse(centre, centre, 2.0 * scale), &solid);
        Ok(())
    })
}

/// Rasterises an SVG with Direct2D, so the cursor and tray icon stay sharp at any display scale.
unsafe fn svg_icon(
    gfx: &Graphics,
    svg: &str,
    size: u32,
    hotspot: Option<(u32, u32)>,
) -> Result<HICON> {
    let stream = SHCreateMemStream(Some(svg.as_bytes()))
        .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let dc5: ID2D1DeviceContext5 = gfx.dc.cast()?;
    let document = dc5.CreateSvgDocument(
        &stream,
        D2D_SIZE_F {
            width: size as f32,
            height: size as f32,
        },
    )?;
    render_icon(gfx, size, hotspot, |_| {
        dc5.DrawSvgDocument(&document);
        Ok(())
    })
}

/// Draws into an offscreen bitmap and turns it into an icon, or a cursor if given a hotspot.
unsafe fn render_icon(
    gfx: &Graphics,
    size: u32,
    hotspot: Option<(u32, u32)>,
    draw: impl FnOnce(&ID2D1DeviceContext) -> Result<()>,
) -> Result<HICON> {
    let extent = D2D_SIZE_U {
        width: size,
        height: size,
    };
    let canvas = gfx.dc.CreateBitmap(
        extent,
        None,
        0,
        &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET),
    )?;
    let readback = gfx.dc.CreateBitmap(
        extent,
        None,
        0,
        &bitmap_properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW),
    )?;
    gfx.dc.SetTarget(&canvas);
    gfx.dc.BeginDraw();
    gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
    let painted = draw(&gfx.dc);
    let drawn = gfx.dc.EndDraw(None, None);
    if let Some(target) = &gfx.target {
        gfx.dc.SetTarget(target);
    }
    painted?;
    drawn?;
    readback.CopyFromBitmap(None, &canvas, None)?;

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size as i32,
        biHeight: -(size as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let color = CreateDIBSection(
        None,
        &BITMAPINFO {
            bmiHeader: header,
            ..Default::default()
        },
        DIB_RGB_COLORS,
        &mut bits,
        None,
        0,
    )?;
    let mapped = readback.Map(D2D1_MAP_OPTIONS_READ)?;
    let row_bytes = size as usize * 4;
    for row in 0..size as usize {
        let source =
            std::slice::from_raw_parts(mapped.bits.add(row * mapped.pitch as usize), row_bytes);
        let destination =
            std::slice::from_raw_parts_mut((bits as *mut u8).add(row * row_bytes), row_bytes);
        // Direct2D hands back premultiplied pixels; icon bitmaps want straight alpha.
        for (from, to) in source
            .as_chunks::<4>()
            .0
            .iter()
            .zip(destination.as_chunks_mut::<4>().0)
        {
            let alpha = from[3];
            for (channel, value) in to.iter_mut().take(3).enumerate() {
                *value = if alpha == 0 {
                    0
                } else {
                    (u32::from(from[channel]) * 255 / u32::from(alpha)).min(255) as u8
                };
            }
            to[3] = alpha;
        }
    }
    readback.Unmap()?;

    let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
    let (x, y) = hotspot.unwrap_or_default();
    let icon = CreateIconIndirect(&ICONINFO {
        fIcon: hotspot.is_none().into(),
        xHotspot: x,
        yHotspot: y,
        hbmMask: mask,
        hbmColor: color,
    });
    let _ = DeleteObject(color.into());
    let _ = DeleteObject(mask.into());
    icon
}
