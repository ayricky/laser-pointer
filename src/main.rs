//! Screen-wide laser pointer, driven by user-configurable shortcuts (Alt+L toggles it by default).
//! While on: left-drag draws; right clicks and the scroll wheel pass through to the app underneath.
//! A tray icon opens the settings panel.
#![windows_subsystem = "windows"]

mod frame;
mod motion;
mod panel;
mod panel_graphics;
mod settings;
mod stroke;
mod ui;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, COLORREF, D2DERR_RECREATE_TARGET, ERROR_ALREADY_EXISTS, E_FAIL,
    HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F,
    D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1DeviceContext5, ID2D1Factory1,
    ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_MAP_OPTIONS_READ, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
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
    CreateDXGIFactory2, IDXGIDevice, IDXGIDevice3, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
    IDXGISwapChain2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
    DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetMonitorInfoW, MonitorFromPoint, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetMouseMovePointsEx, RegisterHotKey, ReleaseCapture, SendInput, SetCapture, UnregisterHotKey,
    GMMP_USE_DISPLAY_POINTS, HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_MOUSE, MOD_NOREPEAT,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    MOUSEMOVEPOINT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, CreateWindowExW, DefWindowProcW, DestroyCursor, DispatchMessageW,
    GetCursor, GetCursorPos, GetMessageTime, GetSystemMetrics, GetWindowLongPtrW, KillTimer,
    LoadCursorW, MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, RegisterClassW,
    SetCursor, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, GWL_EXSTYLE, HCURSOR, HICON, HWND_TOPMOST, ICONINFO, IDC_CROSS, LWA_ALPHA,
    MA_NOACTIVATE, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_CANCELMODE, WM_CAPTURECHANGED, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_numerics::Vector2;

use settings::{Binding, Command, Slot, COMMANDS, MAX_PRESETS};
use stroke::{rgba, union, Bounds, Point, Scratch, Stroke, MIN_STEP};

// Hotkey ids follow COMMANDS; presets start here.
const HOTKEY_PRESET: i32 = 100;
const HOTKEY_PROBE: i32 = 99;
const TIMER_PASSTHROUGH: usize = 1;
const TIMER_RECOVER: usize = 2;
const TIMER_TRIM: usize = 3;
const WM_REFRESH_TRAY: u32 = 0x8001;
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
    scale: f32,
}

struct Toast {
    layout: IDWriteTextLayout,
    width: f32,
    height: f32,
    centre: f32,
    bottom: f32,
    at: Instant,
    scale: f32,
}

struct InkLayer {
    bitmap: ID2D1Bitmap1,
    bounds: Option<Bounds>,
}

impl InkLayer {
    fn matches_size(&self, size: (u32, u32)) -> bool {
        let pixels = unsafe { self.bitmap.GetPixelSize() };
        (pixels.width, pixels.height) == size
    }
}

struct FadingInk {
    strokes: Vec<Stroke>,
    at: Instant,
}

enum Edit {
    Stroke(Stroke),
    Clear,
}

struct Graphics {
    swap_chain: IDXGISwapChain1,
    waitable: HANDLE,
    factory: ID2D1Factory1,
    dc: ID2D1DeviceContext,
    target: Option<ID2D1Bitmap1>,
    brush: ID2D1SolidColorBrush,
    dwrite: IDWriteFactory,
    toast_formats: Vec<(u32, IDWriteTextFormat)>,
    dxgi: IDXGIDevice3,
    ink: Option<InkLayer>,
    fading: Option<InkLayer>,
    /// Transferred ink already contains the baked strokes of a new fade.
    fading_snapshot: bool,
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
    gfx: Option<Graphics>,
    frame_slot: bool,
    strokes: Vec<Stroke>,
    redo: Vec<Edit>,
    cleared: Option<Vec<Stroke>>,
    fading: Option<FadingInk>,
    ink_rebuild: bool,
    fade_rebuild: bool,
    trimmed: bool,
    history: Option<MouseSample>,
    drawing: bool,
    dirty: bool,
    /// Virtual-screen position of the window, to map screen coordinates into it.
    origin: (i32, i32),
    ripples: Vec<Ripple>,
    toast: Option<Toast>,
    shown: bool,
    /// What each swap-chain buffer last had drawn on it, so a frame only clears and
    /// presents the parts of the screen that changed.
    buffers: [DirtyRects; 2],
    previous_dynamic: DirtyRects,
    back: usize,
    presented: bool,
    scratch: Scratch,
    cursor_key: Option<(u32, bool, u32)>,
    cursors: Vec<((u32, bool, u32), HCURSOR)>,
}

struct UiApartment;

impl Drop for UiApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn main() -> Result<()> {
    unsafe {
        let _single_instance = CreateMutexW(None, false, w!("Local\\LaserPointer"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            ui::show_running_instance();
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _apartment = match CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok() {
            Ok(()) => Some(UiApartment),
            Err(error) => {
                eprintln!("Could not initialise the UI COM apartment: {error}");
                None
            }
        };
        let first_run = settings::load();
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

        let gfx = Graphics::new(hwnd, width as u32, height as u32).ok();

        // Stays hidden until there's something to show, so nothing sits over the desktop.
        APP.with(|app| *app.borrow_mut() = Some(App::new(hwnd, gfx, (x, y))));
        with_app(App::refresh_cursor);
        with_app(|app| {
            app.reset_buffers();
            if app.gfx.is_none() {
                app.schedule_recovery();
            }
            SetTimer(Some(hwnd), TIMER_TRIM, 3000, None);
        });
        // Shortcuts first: adding the tray icon can take Explorer a few seconds.
        sync_hotkeys();
        ui::init(instance.into())?;
        if first_run {
            if let Err(error) = ui::welcome() {
                eprintln!("Could not open the welcome panel: {error}");
            }
            let shortcut = ui::shortcut_name(settings::get().key(Command::Toggle));
            with_app(|app| app.notify(&format!("Laser Pointer is ready — {shortcut} to draw")));
            settings::save();
        }
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
        let overlay_schedule = with_app(|app| frame::schedule(app.needs_frame(), app.frame_slot))
            .unwrap_or(frame::Schedule::Idle);
        let overlay_waitable = if overlay_schedule == frame::Schedule::Wait {
            with_app(|app| app.gfx.as_ref().map(|gfx| gfx.waitable)).flatten()
        } else {
            None
        };
        let panel_schedule = ui::frame_schedule();
        let panel_waitable = ui::animation_waitable();
        let mut handles = [HANDLE::default(); 2];
        let mut count = 0;
        let overlay_index = overlay_waitable.map(|handle| {
            handles[count] = handle;
            count += 1;
            0
        });
        let panel_index = panel_waitable.map(|handle| {
            let index = count;
            handles[index] = handle;
            count += 1;
            index
        });
        let timeout = if overlay_schedule == frame::Schedule::Render
            || panel_schedule == frame::Schedule::Render
        {
            0
        } else {
            INFINITE
        };
        let woke = MsgWaitForMultipleObjectsEx(
            Some(&handles[..count]),
            timeout,
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        );
        for (index, handle) in handles[..count].iter().enumerate() {
            let signalled = woke.0 == WAIT_OBJECT_0.0 + index as u32
                || windows::Win32::System::Threading::WaitForSingleObject(*handle, 0)
                    == WAIT_OBJECT_0;
            if signalled {
                if overlay_index == Some(index) {
                    with_app(|app| app.frame_slot = true);
                } else if panel_index == Some(index) {
                    ui::hold_frame_slot();
                }
            }
        }
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        with_app(|app| {
            if frame::schedule(app.needs_frame(), app.frame_slot) == frame::Schedule::Render {
                app.render();
            }
        });
        if ui::frame_schedule() == frame::Schedule::Render {
            ui::render_panel();
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

pub fn remove_kept_strokes() {
    with_app(App::remove_kept_strokes);
}

pub fn run_command(command: Command) {
    match command {
        Command::Toggle => set_laser(!laser_on()),
        Command::Off => set_laser(false),
        Command::Undo => {
            with_app(App::undo);
        }
        Command::Redo => {
            with_app(App::redo);
        }
        Command::Clear => {
            with_app(App::clear);
        }
        Command::ToggleInk => {
            let mut current = settings::get();
            current.look.ink = !current.look.ink;
            settings::set(current);
            settings::save();
            if !current.look.ink {
                remove_kept_strokes();
            }
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
                let step = if command == Command::NextPreset {
                    1
                } else {
                    count - 1
                };
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
                Command::Off | Command::Undo | Command::Redo => on,
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
    with_app(|app| unsafe { svg_icon(app.gfx.as_ref()?, &svg, size, None).ok() })?
}

fn cursor_svg() -> String {
    format!(
        r##"<svg viewBox="0 0 24 24" stroke-width="1" xmlns="http://www.w3.org/2000/svg"><path stroke="#1b1b1f" fill="#fff" d="{PEN_PATH}"/></svg>"##
    )
}

/// The taskbar uses its own theme, independently of app windows.
fn tray_svg(on: bool, color: u32) -> String {
    let light = panel::read_dword(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("SystemUsesLightTheme"),
    )
    .is_some_and(|value| value != 0);
    let source = if on {
        include_str!("../assets/brand/tray-on.svg")
    } else {
        include_str!("../assets/brand/tray.svg")
    };
    source
        .replace("INK", if light { "#1b1b1b" } else { "#ffffff" })
        .replace("DOT", &format!("#{color:06x}"))
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
                with_app(App::refresh_cursor);
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
                with_app(App::refresh_cursor);
                with_app(|app| app.extend(client_point(lparam)));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_app(|app| {
                    app.extend(client_point(lparam));
                    app.finish();
                });
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_RBUTTONDOWN => LRESULT(0),
            WM_MBUTTONDOWN | WM_XBUTTONDOWN => LRESULT(isize::from(msg == WM_XBUTTONDOWN)),
            WM_MBUTTONUP | WM_XBUTTONUP => {
                with_app(|app| app.ripple(client_point(lparam)));
                let (down, up, data) = if msg == WM_MBUTTONUP {
                    (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0)
                } else {
                    (
                        MOUSEEVENTF_XDOWN,
                        MOUSEEVENTF_XUP,
                        ((wparam.0 >> 16) & 0xffff) as u32,
                    )
                };
                pass_through(hwnd, &[mouse_input(down, data), mouse_input(up, data)]);
                LRESULT(isize::from(msg == WM_XBUTTONUP))
            }
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
            WM_MOUSEHWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
                pass_through(
                    hwnd,
                    &[mouse_input(MOUSEEVENTF_HWHEEL, delta as i32 as u32)],
                );
                LRESULT(0)
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                with_app(App::finish);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_PASSTHROUGH => {
                let _ = KillTimer(Some(hwnd), TIMER_PASSTHROUGH);
                if laser_on() {
                    set_click_through(hwnd, false);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_RECOVER => {
                with_app(App::recover);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_TRIM => {
                with_app(App::trim);
                LRESULT(0)
            }
            WM_REFRESH_TRAY => {
                ui::settings_changed();
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
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
    fn new(hwnd: HWND, gfx: Option<Graphics>, origin: (i32, i32)) -> Self {
        Self {
            hwnd,
            gfx,
            frame_slot: false,
            strokes: Vec::new(),
            redo: Vec::new(),
            cleared: None,
            fading: None,
            ink_rebuild: true,
            fade_rebuild: false,
            trimmed: false,
            history: None,
            drawing: false,
            dirty: false,
            origin,
            ripples: Vec::new(),
            toast: None,
            shown: false,
            buffers: [DirtyRects::default(); 2],
            previous_dynamic: DirtyRects::default(),
            back: 0,
            presented: false,
            scratch: Scratch::default(),
            cursor_key: None,
            cursors: Vec::new(),
        }
    }

    fn needs_frame(&self) -> bool {
        self.shown
            && self.gfx.is_some()
            && !self.trimmed
            && (self.dirty
                || self
                    .strokes
                    .iter()
                    .any(|stroke| !stroke.look.ink && !stroke.points.is_empty())
                || self.fading.is_some()
                || !self.ripples.is_empty()
                || self.toast.is_some())
    }

    fn has_content(&self) -> bool {
        !self.strokes.is_empty()
            || self.fading.is_some()
            || !self.ripples.is_empty()
            || self.toast.is_some()
    }

    fn update_ink_visible(&self) {
        INK_VISIBLE.store(
            self.strokes.iter().any(|stroke| stroke.look.ink),
            Ordering::Relaxed,
        );
    }

    fn remove_kept_strokes(&mut self) {
        self.finish();
        let mut kept = Vec::new();
        let mut index = 0;
        while index < self.strokes.len() {
            if self.strokes[index].look.ink {
                kept.push(self.strokes.remove(index));
            } else {
                index += 1;
            }
        }
        self.start_fade(kept);
        self.ink_rebuild = true;
        self.update_ink_visible();
    }

    fn set_on(&mut self, on: bool) {
        self.finish();
        LASER_ON.store(on, Ordering::Relaxed);
        if !settings::get().look.ink {
            self.strokes.retain(|stroke| stroke.look.ink);
        }
        self.ripples.clear();
        self.dirty = true;
        self.toast = None;
        self.notify(if on { "Laser on" } else { "Laser off" });
        unsafe {
            set_click_through(self.hwnd, !on);
            if on {
                self.show();
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

    /// Paint before showing, including restoring buffers released during a long idle.
    fn show(&mut self) {
        if self.shown {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TRIM);
        }
        if self.trimmed {
            let (_, _, width, height) = unsafe { virtual_screen() };
            let resized = self
                .gfx
                .as_mut()
                .map(|gfx| unsafe { gfx.resize(width as u32, height as u32) });
            self.trimmed = false;
            if let Some(Err(error)) = resized {
                self.graphics_failed(error);
            }
            self.reset_buffers();
            self.ink_rebuild = true;
            self.fade_rebuild = true;
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
            SetTimer(Some(self.hwnd), TIMER_TRIM, 3000, None);
        }
        self.shown = false;
    }

    fn notify(&mut self, text: &str) {
        if !settings::get().indicator {
            return;
        }
        self.toast = self.make_toast(text);
        self.dirty = true;
        self.show();
    }

    fn make_toast(&mut self, text: &str) -> Option<Toast> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut cursor = POINT::default();
        unsafe {
            GetCursorPos(&mut cursor).ok()?;
        }
        let dpi = monitor_dpi(cursor);
        let scale = dpi as f32 / 96.0;
        let gfx = self.gfx.as_mut()?;
        unsafe {
            let format = gfx.toast_format(dpi).ok()?;
            let layout = gfx
                .dwrite
                .CreateTextLayout(&wide, &format, 4000.0, 400.0)
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(
                MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST),
                &mut info,
            )
            .as_bool()
            {
                return None;
            }
            let work = info.rcWork;
            Some(Toast {
                layout,
                width: metrics.width,
                height: metrics.height,
                centre: ((work.left + work.right) / 2 - self.origin.0) as f32,
                bottom: (work.bottom - self.origin.1) as f32 - 72.0 * scale,
                at: Instant::now(),
                scale,
            })
        }
    }

    fn screen_point(&self, (x, y): (f32, f32)) -> POINT {
        POINT {
            x: x as i32 + self.origin.0,
            y: y as i32 + self.origin.1,
        }
    }

    fn begin(&mut self, (x, y): (f32, f32)) {
        if !laser_on() {
            return;
        }
        self.finish();
        self.redo.clear();
        self.cleared = None;
        self.drawing = true;
        self.dirty = true;
        let screen = self.screen_point((x, y));
        let time = unsafe { GetMessageTime() as u32 };
        self.history = Some(MouseSample {
            x: screen.x,
            y: screen.y,
            time,
        });
        self.strokes.push(Stroke {
            points: vec![Point {
                x,
                y,
                at: sample_instant(time, unsafe { GetTickCount() }, Instant::now()),
            }],
            look: settings::get().look,
            scale: monitor_dpi(screen) as f32 / 96.0,
            baked: false,
        });
        self.update_ink_visible();
    }

    fn append(&mut self, sample: MouseSample, now: Instant, ticks: u32) {
        self.history = Some(sample);
        let Some(stroke) = self.strokes.last_mut() else {
            return;
        };
        let (x, y) = (
            (sample.x - self.origin.0) as f32,
            (sample.y - self.origin.1) as f32,
        );
        if stroke
            .points
            .last()
            .is_some_and(|last| (x - last.x).hypot(y - last.y) < MIN_STEP * stroke.scale)
        {
            return;
        }
        let at = sample_instant(sample.time, ticks, now);
        let at = stroke.points.last().map_or(at, |last| at.max(last.at));
        stroke.points.push(Point { x, y, at });
        self.dirty = true;
    }

    fn extend(&mut self, point: (f32, f32)) {
        if !self.drawing {
            return;
        }
        let screen = self.screen_point(point);
        let time = unsafe { GetMessageTime() as u32 };
        let current = MouseSample {
            x: screen.x,
            y: screen.y,
            time,
        };
        let query = MOUSEMOVEPOINT {
            x: screen.x & 0xffff,
            y: screen.y & 0xffff,
            time,
            ..Default::default()
        };
        let mut raw = [MOUSEMOVEPOINT::default(); 64];
        let count = unsafe {
            GetMouseMovePointsEx(
                std::mem::size_of::<MOUSEMOVEPOINT>() as u32,
                &query,
                &mut raw,
                GMMP_USE_DISPLAY_POINTS,
            )
        };
        let (now, ticks) = (Instant::now(), unsafe { GetTickCount() });
        if count > 0 {
            let mut samples = [MouseSample::default(); 64];
            let count = count as usize;
            for (sample, raw) in samples[..count].iter_mut().zip(&raw[..count]) {
                *sample = MouseSample {
                    x: unwrap_coordinate(raw.x),
                    y: unwrap_coordinate(raw.y),
                    time: raw.time,
                };
            }
            let end = history_count(&samples[..count], self.history);
            for &sample in samples[..end].iter().rev() {
                self.append(sample, now, ticks);
            }
        }
        if self.history != Some(current) {
            self.append(current, now, ticks);
        }
    }

    fn finish(&mut self) {
        if self.drawing {
            self.drawing = false;
            self.history = None;
            self.dirty = true;
        }
    }

    fn undo(&mut self) {
        if self.drawing {
            return;
        }
        if let Some(strokes) = self.cleared.take() {
            self.strokes = strokes;
            self.cancel_fade();
            self.redo.push(Edit::Clear);
        } else if let Some(stroke) = self.strokes.pop() {
            if let Some(bounds) = stroke.bounds() {
                self.invalidate(bounds);
            }
            self.redo.push(Edit::Stroke(stroke));
        }
        self.ink_rebuild = true;
        self.dirty = true;
        self.update_ink_visible();
    }

    fn redo(&mut self) {
        if self.drawing {
            return;
        }
        match self.redo.pop() {
            Some(Edit::Stroke(mut stroke)) => {
                stroke.baked = false;
                self.strokes.push(stroke);
            }
            Some(Edit::Clear) => self.clear_strokes(false),
            None => return,
        }
        self.ink_rebuild = true;
        self.dirty = true;
        self.update_ink_visible();
    }

    fn clear(&mut self) {
        self.clear_strokes(true);
    }

    fn clear_strokes(&mut self, clear_redo: bool) {
        self.finish();
        if self.strokes.is_empty() {
            return;
        }
        if clear_redo {
            self.redo.clear();
        }
        let removed = std::mem::take(&mut self.strokes);
        self.cleared = Some(removed.clone());
        self.start_fade(removed);
        self.ink_rebuild = true;
        self.dirty = true;
        self.update_ink_visible();
    }

    fn start_fade(&mut self, strokes: Vec<Stroke>) {
        if strokes.is_empty() {
            return;
        }
        if let Some(gfx) = &mut self.gfx {
            let bounds = gfx.fading.as_ref().and_then(|layer| layer.bounds);
            gfx.fading_snapshot = gfx.ink.is_some() && !self.ink_rebuild;
            if let Some(layer) = gfx.ink.take() {
                gfx.fading = Some(layer);
            }
            if let Some(bounds) = bounds {
                self.invalidate(bounds);
            }
        }
        self.fading = Some(FadingInk {
            strokes,
            at: Instant::now(),
        });
        self.fade_rebuild = true;
        self.dirty = true;
    }

    fn cancel_fade(&mut self) {
        if let Some(gfx) = &mut self.gfx {
            let layer = gfx.fading.take();
            let bounds = layer.as_ref().and_then(|layer| layer.bounds);
            if gfx.ink.is_none() && self.strokes.iter().any(|stroke| stroke.look.ink) {
                gfx.ink = layer;
                self.ink_rebuild = true;
            }
            gfx.fading_snapshot = false;
            if let Some(bounds) = bounds {
                self.invalidate(bounds);
            }
        }
        self.fading = None;
    }

    fn ripple(&mut self, (x, y): (f32, f32)) {
        if settings::get().ripples {
            self.ripples.push(Ripple {
                x,
                y,
                at: Instant::now(),
                scale: monitor_dpi(self.screen_point((x, y))) as f32 / 96.0,
            });
            self.dirty = true;
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
        let offset = (self.origin.0 - x, self.origin.1 - y);
        for stroke in &mut self.strokes {
            stroke.translate(offset);
        }
        if let Some(strokes) = &mut self.cleared {
            for stroke in strokes {
                stroke.translate(offset);
            }
        }
        for edit in &mut self.redo {
            match edit {
                Edit::Stroke(stroke) => stroke.translate(offset),
                Edit::Clear => {}
            }
        }
        if let Some(fading) = &mut self.fading {
            for stroke in &mut fading.strokes {
                stroke.translate(offset);
            }
        }
        for ripple in &mut self.ripples {
            ripple.x += offset.0 as f32;
            ripple.y += offset.1 as f32;
        }
        if let Some(toast) = &mut self.toast {
            toast.centre += offset.0 as f32;
            toast.bottom += offset.1 as f32;
        }
        self.origin = (x, y);
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
        }
        if !self.trimmed {
            let result = self
                .gfx
                .as_mut()
                .map(|gfx| unsafe { gfx.resize(width as u32, height as u32) });
            if let Some(Err(error)) = result {
                self.graphics_failed(error);
            }
        }
        self.reset_buffers();
        self.ink_rebuild = true;
        self.fade_rebuild = true;
        self.refresh_cursor();
        self.dirty = true;
    }

    fn refresh_cursor(&mut self) {
        let settings = settings::get();
        let mut pointer = POINT::default();
        if unsafe { GetCursorPos(&mut pointer) }.is_err() {
            return;
        }
        let dpi = monitor_dpi(pointer);
        let key = (settings.look.color, settings.dot_cursor, dpi);
        if self.cursor_key == Some(key) {
            return;
        }
        let cursor =
            if let Some((_, cursor)) = self.cursors.iter().find(|(cached, _)| *cached == key) {
                *cursor
            } else {
                let Some(gfx) = &self.gfx else { return };
                let scale = dpi as f32 / 96.0;
                let made = unsafe {
                    if settings.dot_cursor {
                        dot_cursor(gfx, settings.look.color, scale)
                    } else {
                        svg_icon(
                            gfx,
                            &cursor_svg(),
                            (CURSOR_SIZE * scale).round() as u32,
                            Some((0, 0)),
                        )
                    }
                };
                match made {
                    Ok(icon) => {
                        let cursor = HCURSOR(icon.0);
                        self.cursors.push((key, cursor));
                        cursor
                    }
                    Err(error) => {
                        if device_lost(&error) {
                            self.graphics_failed(error);
                            return;
                        }
                        unsafe { LoadCursorW(None, IDC_CROSS).unwrap_or_default() }
                    }
                }
            };
        self.cursor_key = Some(key);
        CURSOR.store(cursor.0 as isize, Ordering::Relaxed);
        let active = unsafe { GetCursor() };
        if laser_on() || self.cursors.iter().any(|(_, cached)| *cached == active) {
            unsafe {
                SetCursor(Some(cursor));
            }
        }
        retain_cursor_look(&mut self.cursors, key, |cursor| unsafe {
            let _ = DestroyCursor(cursor);
        });
    }

    fn reset_buffers(&mut self) {
        self.buffers = [DirtyRects::default(); 2];
        if let Some(gfx) = &self.gfx {
            let bounds = full_bounds(gfx.size.0, gfx.size.1);
            self.invalidate(bounds);
        }
        self.previous_dynamic = DirtyRects::default();
        self.back = 0;
        self.presented = false;
    }

    fn invalidate(&mut self, bounds: Bounds) {
        if let Some(gfx) = &self.gfx {
            let rectangle = pixel_rect(bounds, gfx.size, 0.0);
            for buffer in &mut self.buffers {
                buffer.add(rectangle);
            }
        }
    }

    fn schedule_recovery(&self) {
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_RECOVER, 1000, None);
        }
    }

    /// Release the composition target before attaching another tree to this HWND.
    fn graphics_failed(&mut self, error: windows::core::Error) {
        eprintln!("Overlay graphics will be rebuilt: {error}");
        self.gfx = None;
        self.frame_slot = false;
        self.cursor_key = None;
        self.ink_rebuild = true;
        self.fade_rebuild = true;
        self.reset_buffers();
        self.schedule_recovery();
    }

    fn recover(&mut self) {
        if self.gfx.is_some() {
            return;
        }
        let (_, _, width, height) = unsafe { virtual_screen() };
        let (width, height) = if self.trimmed {
            (8, 8)
        } else {
            (width as u32, height as u32)
        };
        match unsafe { Graphics::new(self.hwnd, width, height) } {
            Ok(gfx) => {
                self.gfx = Some(gfx);
                self.frame_slot = false;
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_RECOVER);
                }
                self.cursor_key = None;
                self.release_cursors();
                self.refresh_cursor();
                self.reset_buffers();
                self.dirty = true;
                // Tray rasterisation borrows APP, so do it once this handler has returned.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(self.hwnd),
                        WM_REFRESH_TRAY,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            Err(error) => eprintln!("Overlay graphics rebuild failed: {error}"),
        }
    }

    fn release_cursors(&mut self) {
        if self.cursors.is_empty() {
            return;
        }
        unsafe {
            let fallback = LoadCursorW(None, IDC_CROSS).unwrap_or_default();
            CURSOR.store(fallback.0 as isize, Ordering::Relaxed);
            if laser_on() {
                SetCursor(Some(fallback));
            }
            for (_, cursor) in self.cursors.drain(..) {
                let _ = DestroyCursor(cursor);
            }
        }
    }

    fn trim(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TRIM);
        }
        if self.shown || self.trimmed {
            return;
        }
        self.trimmed = true;
        let result = self.gfx.as_mut().map(|gfx| unsafe {
            gfx.ink = None;
            gfx.fading = None;
            gfx.resize(8, 8)?;
            gfx.dxgi.Trim();
            Ok(())
        });
        if let Some(Err(error)) = result {
            self.graphics_failed(error);
        }
        self.reset_buffers();
        self.ink_rebuild = true;
        self.fade_rebuild = true;
    }

    fn render(&mut self) {
        if self.gfx.is_none() || self.trimmed {
            return;
        }
        if let Err(error) = self.render_frame() {
            if device_lost(&error) {
                self.graphics_failed(error);
            } else {
                eprintln!("Could not draw overlay frame: {error}");
                self.retry_frame();
            }
            return;
        }
        let was_visible = INK_VISIBLE.load(Ordering::Relaxed);
        self.update_ink_visible();
        if was_visible != INK_VISIBLE.load(Ordering::Relaxed) {
            sync_hotkeys();
        }
        if !laser_on() && !self.has_content() && self.shown {
            self.hide();
        }
    }

    /// Retry partial bakes from a clear bitmap so translucent strokes aren't blended twice.
    fn retry_frame(&mut self) {
        self.reset_buffers();
        self.ink_rebuild = true;
        self.fade_rebuild = true;
        if let Some(gfx) = &mut self.gfx {
            gfx.fading_snapshot = false;
        }
        self.dirty = true;
    }

    fn prune_content(&mut self, now: Instant) {
        self.ripples
            .retain(|ripple| now.duration_since(ripple.at).as_secs_f32() < RIPPLE_SECS);
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| now.duration_since(toast.at).as_secs_f32() >= TOAST_SECS)
        {
            self.toast = None;
        }
        if self
            .fading
            .as_ref()
            .is_some_and(|fade| now.duration_since(fade.at) >= Duration::from_millis(250))
        {
            self.cancel_fade();
        }
        for stroke in &mut self.strokes {
            if !stroke.look.ink {
                stroke.prune(now, &mut self.scratch);
            }
        }
        let active = self.drawing.then(|| self.strokes.len().saturating_sub(1));
        let mut index = 0;
        self.strokes.retain(|stroke| {
            let keep = !stroke.points.is_empty() || active == Some(index);
            index += 1;
            keep
        });
    }

    fn bake_ink(&mut self, now: Instant) -> Result<Option<Bounds>> {
        let active = self.drawing.then(|| self.strokes.len().saturating_sub(1));
        let Some(gfx) = &mut self.gfx else {
            return Ok(None);
        };
        let has_kept = self
            .strokes
            .iter()
            .enumerate()
            .any(|(index, stroke)| stroke.look.ink && active != Some(index));
        if gfx
            .ink
            .as_ref()
            .is_some_and(|layer| !layer.matches_size(gfx.size))
        {
            self.ink_rebuild = true;
        }
        let mut ink_damage = None;
        if self.ink_rebuild {
            ink_damage = gfx.ink.as_ref().and_then(|layer| layer.bounds);
            if has_kept {
                let mut layer = gfx.ink.take();
                let cleared = unsafe { gfx.reset_layer(&mut layer) };
                gfx.ink = layer;
                cleared?;
            }
            for stroke in &mut self.strokes {
                stroke.baked = false;
            }
        }
        if !has_kept {
            ink_damage = union(ink_damage, gfx.ink.take().and_then(|layer| layer.bounds));
        } else if self
            .strokes
            .iter()
            .enumerate()
            .any(|(index, stroke)| stroke.look.ink && active != Some(index) && !stroke.baked)
        {
            if gfx.ink.is_none() {
                gfx.ink = Some(unsafe { gfx.empty_layer()? });
            }
            let layer = gfx
                .ink
                .as_mut()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            unsafe {
                gfx.dc.SetTarget(&layer.bitmap);
                gfx.dc.BeginDraw();
            }
            let mut painted = Ok(());
            for (index, stroke) in self.strokes.iter_mut().enumerate() {
                if stroke.look.ink && active != Some(index) && !stroke.baked {
                    match stroke::draw(
                        &gfx.factory,
                        &gfx.dc,
                        &gfx.brush,
                        stroke,
                        now,
                        &mut self.scratch,
                    ) {
                        Ok(bounds) => {
                            layer.bounds = union(layer.bounds, bounds);
                            ink_damage =
                                union(ink_damage, bounds.map(|bounds| inflate(bounds, 2.0)));
                            stroke.baked = true;
                        }
                        Err(error) => {
                            painted = Err(error);
                            break;
                        }
                    }
                }
            }
            let drawn = unsafe { gfx.dc.EndDraw(None, None) };
            unsafe {
                gfx.restore_target();
            }
            painted?;
            drawn?;
        }
        self.ink_rebuild = false;

        Ok(ink_damage)
    }

    fn bake_fading(&mut self) -> Result<()> {
        if !self.fade_rebuild {
            return Ok(());
        }
        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };
        if let Some(fade) = &mut self.fading {
            let mut layer = gfx.fading.take();
            let reused = gfx.fading_snapshot
                && layer
                    .as_ref()
                    .is_some_and(|layer| layer.matches_size(gfx.size));
            gfx.fading_snapshot = false;
            let cleared = if reused {
                Ok(())
            } else {
                unsafe { gfx.reset_layer(&mut layer) }
            };
            gfx.fading = layer;
            cleared?;
            let layer = gfx
                .fading
                .as_mut()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            unsafe {
                gfx.dc.SetTarget(&layer.bitmap);
                gfx.dc.BeginDraw();
            }
            let mut painted = Ok(());
            for stroke in &mut fade.strokes {
                if reused && stroke.baked {
                    continue;
                }
                match stroke::draw(
                    &gfx.factory,
                    &gfx.dc,
                    &gfx.brush,
                    stroke,
                    fade.at,
                    &mut self.scratch,
                ) {
                    Ok(bounds) => layer.bounds = union(layer.bounds, bounds),
                    Err(error) => {
                        painted = Err(error);
                        break;
                    }
                }
            }
            let drawn = unsafe { gfx.dc.EndDraw(None, None) };
            unsafe {
                gfx.restore_target();
            }
            painted?;
            drawn?;
        }

        self.fade_rebuild = false;
        Ok(())
    }

    fn dynamic_bounds(&self) -> DirtyRects {
        let Some(gfx) = &self.gfx else {
            return DirtyRects::default();
        };
        let mut dynamic = DirtyRects::default();
        for stroke in self.strokes.iter().filter(|stroke| !stroke.baked) {
            if let Some(bounds) = stroke.bounds() {
                dynamic.add(pixel_rect(bounds, gfx.size, 0.0));
            }
        }
        for ripple in &self.ripples {
            dynamic.add(pixel_rect(
                Bounds::around(ripple.x, ripple.y, 40.0 * ripple.scale),
                gfx.size,
                0.0,
            ));
        }
        if let Some(toast) = &self.toast {
            dynamic.add(pixel_rect(toast_bounds(toast), gfx.size, 2.0));
        }
        if let Some(bounds) = gfx.fading.as_ref().and_then(|layer| layer.bounds) {
            dynamic.add(pixel_rect(bounds, gfx.size, 2.0));
        }

        dynamic
    }

    fn paint_regions(&mut self, repaint: &DirtyRects, now: Instant) -> Result<()> {
        let Some(gfx) = &self.gfx else { return Ok(()) };
        let colour = settings::get().look.color;
        unsafe {
            gfx.dc.BeginDraw();
        }
        let mut painted = Ok(());
        for rectangle in repaint.as_slice() {
            let bounds = rect_bounds(*rectangle);
            unsafe {
                gfx.dc
                    .PushAxisAlignedClip(&rect_f(bounds), D2D1_ANTIALIAS_MODE_ALIASED);
                gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
                if let Some(layer) = &gfx.ink {
                    draw_layer(&gfx.dc, layer, bounds, 1.0);
                }
                if let (Some(layer), Some(fade)) = (&gfx.fading, &self.fading) {
                    let k = (now.duration_since(fade.at).as_secs_f32() / 0.25).clamp(0.0, 1.0);
                    draw_layer(&gfx.dc, layer, bounds, (1.0 - k).powi(3));
                }
            }
            for stroke in self.strokes.iter_mut().filter(|stroke| !stroke.baked) {
                if stroke.bounds().is_some_and(|area| intersects(area, bounds)) {
                    if let Err(error) = stroke::draw(
                        &gfx.factory,
                        &gfx.dc,
                        &gfx.brush,
                        stroke,
                        now,
                        &mut self.scratch,
                    ) {
                        painted = Err(error);
                        break;
                    }
                }
            }
            unsafe {
                for ripple in &self.ripples {
                    let k = now.duration_since(ripple.at).as_secs_f32() / RIPPLE_SECS;
                    let radius = (6.0 + 30.0 * (1.0 - (1.0 - k).powi(3))) * ripple.scale;
                    gfx.brush.SetColor(&rgba(colour, 0.9 * (1.0 - k).powf(1.5)));
                    gfx.dc.DrawEllipse(
                        &ellipse(ripple.x, ripple.y, radius),
                        &gfx.brush,
                        3.0 * ripple.scale * (1.0 - 0.5 * k),
                        None,
                    );
                }
                if let Some(toast) = &self.toast {
                    let age = now.duration_since(toast.at).as_secs_f32();
                    let alpha = (age / 0.15).min(1.0).min((TOAST_SECS - age) / 0.3).max(0.0);
                    gfx.draw_toast(toast, colour, alpha, toast.scale);
                }
                gfx.dc.PopAxisAlignedClip();
            }
            if painted.is_err() {
                break;
            }
        }
        let drawn = unsafe { gfx.dc.EndDraw(None, None) };
        painted?;
        drawn?;

        Ok(())
    }

    fn render_frame(&mut self) -> Result<()> {
        let now = Instant::now();
        self.prune_content(now);
        let ink_damage = self.bake_ink(now)?;
        self.bake_fading()?;
        let dynamic = self.dynamic_bounds();
        let Some(size) = self.gfx.as_ref().map(|gfx| gfx.size) else {
            return Ok(());
        };
        for buffer in &mut self.buffers {
            buffer.extend(&self.previous_dynamic);
            buffer.extend(&dynamic);
            if let Some(bounds) = ink_damage {
                buffer.add(pixel_rect(bounds, size, 2.0));
            }
        }
        let mut repaint = if self.presented {
            self.buffers[self.back]
        } else {
            DirtyRects::whole(size)
        };
        if repaint.count == 0 {
            self.dirty = false;
            return Ok(());
        }
        self.paint_regions(&repaint, now)?;
        let parameters = DXGI_PRESENT_PARAMETERS {
            DirtyRectsCount: if self.presented {
                repaint.count as u32
            } else {
                0
            },
            pDirtyRects: repaint.rectangles.as_mut_ptr(),
            ..Default::default()
        };
        if let Some(gfx) = &self.gfx {
            unsafe {
                frame::acquire(&mut self.frame_slot, || frame::wait(gfx.waitable))?;
                frame::present(&mut self.frame_slot, || {
                    gfx.swap_chain
                        .Present1(1, DXGI_PRESENT(0), &parameters)
                        .ok()
                })?;
            }
        }
        self.buffers[self.back] = DirtyRects::default();
        self.back ^= 1;
        self.previous_dynamic = dynamic;
        self.presented = true;
        self.dirty = false;
        Ok(())
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.release_cursors();
    }
}
#[derive(Clone, Copy, Default)]
struct DirtyRects {
    rectangles: [RECT; 8],
    count: usize,
}

impl DirtyRects {
    fn whole(size: (u32, u32)) -> Self {
        let mut result = Self::default();
        result.add(pixel_rect(full_bounds(size.0, size.1), size, 0.0));
        result
    }

    fn as_slice(&self) -> &[RECT] {
        &self.rectangles[..self.count]
    }

    fn extend(&mut self, other: &Self) {
        for &rectangle in other.as_slice() {
            self.add(rectangle);
        }
    }

    fn add(&mut self, rectangle: RECT) {
        if rectangle.right <= rectangle.left || rectangle.bottom <= rectangle.top {
            return;
        }
        let mut bounds = rect_bounds(rectangle);
        let mut index = 0;
        while index < self.count {
            if intersects(inflate(bounds, 4.0), rect_bounds(self.rectangles[index])) {
                bounds = bounds.union(rect_bounds(self.rectangles[index]));
                self.count -= 1;
                self.rectangles[index] = self.rectangles[self.count];
                index = 0;
            } else {
                index += 1;
            }
        }
        if self.count == self.rectangles.len() {
            for rectangle in self.as_slice() {
                bounds = bounds.union(rect_bounds(*rectangle));
            }
            self.count = 0;
        }
        self.rectangles[self.count] = RECT {
            left: bounds.left as i32,
            top: bounds.top as i32,
            right: bounds.right as i32,
            bottom: bounds.bottom as i32,
        };
        self.count += 1;
    }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct MouseSample {
    x: i32,
    y: i32,
    time: u32,
}

fn unwrap_coordinate(value: i32) -> i32 {
    if value > 32767 {
        value - 65536
    } else {
        value
    }
}

/// History is newest first; stop at the last observed sample, including tick wraparound.
fn history_count(samples: &[MouseSample], previous: Option<MouseSample>) -> usize {
    let Some(previous) = previous else {
        return samples.len();
    };
    samples
        .iter()
        .position(|sample| {
            *sample == previous || (sample.time.wrapping_sub(previous.time) as i32) < 0
        })
        .unwrap_or(samples.len())
}

fn sample_instant(time: u32, ticks: u32, now: Instant) -> Instant {
    let age = (ticks.wrapping_sub(time) as i32).max(0) as u64;
    now.checked_sub(Duration::from_millis(age)).unwrap_or(now)
}

fn monitor_dpi(point: POINT) -> u32 {
    let (mut x, mut y) = (96, 96);
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) }.is_err() {
        return 96;
    }
    if x == 0 {
        96
    } else {
        x
    }
}

fn device_lost(error: &windows::core::Error) -> bool {
    matches!(
        error.code(),
        D2DERR_RECREATE_TARGET | DXGI_ERROR_DEVICE_REMOVED | DXGI_ERROR_DEVICE_RESET
    )
}

fn retain_cursor_look<T>(
    cursors: &mut Vec<((u32, bool, u32), T)>,
    current: (u32, bool, u32),
    mut release: impl FnMut(T),
) {
    for (_, cursor) in cursors.extract_if(.., |(key, _)| key.0 != current.0 || key.1 != current.1) {
        release(cursor);
    }
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    a.left <= b.right && a.right >= b.left && a.top <= b.bottom && a.bottom >= b.top
}

fn rect_bounds(rectangle: RECT) -> Bounds {
    Bounds {
        left: rectangle.left as f32,
        top: rectangle.top as f32,
        right: rectangle.right as f32,
        bottom: rectangle.bottom as f32,
    }
}

fn toast_bounds(toast: &Toast) -> Bounds {
    let width = toast.width + 56.0 * toast.scale;
    Bounds {
        left: toast.centre - width / 2.0,
        right: toast.centre + width / 2.0,
        top: toast.bottom - toast.height - 18.0 * toast.scale,
        bottom: toast.bottom,
    }
}

unsafe fn draw_layer(dc: &ID2D1DeviceContext, layer: &InkLayer, bounds: Bounds, opacity: f32) {
    if layer.bounds.is_some_and(|area| intersects(area, bounds)) {
        let rectangle = rect_f(bounds);
        dc.DrawBitmap(
            &layer.bitmap,
            Some(&rectangle),
            opacity,
            D2D1_INTERPOLATION_MODE_LINEAR,
            Some(&rectangle),
            None,
        );
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

unsafe fn graphics_device() -> Result<IDXGIDevice> {
    let mut d3d: Option<ID3D11Device> = None;
    let mut result = Err(windows::core::Error::from(E_FAIL));
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        d3d = None;
        result = D3D11CreateDevice(
            None,
            driver,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        );
        if result.is_ok() {
            break;
        }
    }
    result?;
    d3d.ok_or_else(|| windows::core::Error::from(E_FAIL))?
        .cast()
}

impl Graphics {
    unsafe fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        let dxgi = graphics_device()?;

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

        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = d2d
            .CreateDevice(&dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let brush = dc.CreateSolidColorBrush(&rgba(settings::get().look.color, 1.0), None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = composition.CreateTargetForHwnd(hwnd, true)?;
        let visual = composition.CreateVisual()?;
        visual.SetContent(&swap_chain)?;
        target.SetRoot(&visual)?;
        composition.Commit()?;
        let device = dxgi.cast()?;
        let waitable = latency.GetFrameLatencyWaitableObject();
        if waitable.is_invalid() {
            return Err(windows::core::Error::from(E_FAIL));
        }

        let mut gfx = Self {
            swap_chain,
            waitable,
            factory: d2d,
            dc,
            target: None,
            brush,
            dwrite,
            toast_formats: Vec::new(),
            dxgi: device,
            ink: None,
            fading: None,
            fading_snapshot: false,
            size: (width, height),
            _composition: (composition, target, visual),
        };
        gfx.bind_target()?;
        Ok(gfx)
    }

    unsafe fn toast_format(&mut self, dpi: u32) -> Result<IDWriteTextFormat> {
        if let Some((_, format)) = self.toast_formats.iter().find(|(cached, _)| *cached == dpi) {
            return Ok(format.clone());
        }
        let format = self.dwrite.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            15.0 * dpi as f32 / 96.0,
            w!("en-us"),
        )?;
        self.toast_formats.push((dpi, format.clone()));
        Ok(format)
    }

    unsafe fn empty_layer(&self) -> Result<InkLayer> {
        let bitmap = self.dc.CreateBitmap(
            D2D_SIZE_U {
                width: self.size.0,
                height: self.size.1,
            },
            None,
            0,
            &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET),
        )?;
        self.dc.SetTarget(&bitmap);
        self.dc.BeginDraw();
        self.dc.Clear(Some(&D2D1_COLOR_F::default()));
        let drawn = self.dc.EndDraw(None, None);
        self.restore_target();
        drawn?;
        Ok(InkLayer {
            bitmap,
            bounds: None,
        })
    }

    unsafe fn reset_layer(&self, layer: &mut Option<InkLayer>) -> Result<()> {
        if let Some(layer) = layer.as_mut().filter(|layer| layer.matches_size(self.size)) {
            self.dc.SetTarget(&layer.bitmap);
            self.dc.BeginDraw();
            self.dc.Clear(Some(&D2D1_COLOR_F::default()));
            let drawn = self.dc.EndDraw(None, None);
            self.restore_target();
            drawn?;
            layer.bounds = None;
        } else {
            *layer = Some(self.empty_layer()?);
        }
        Ok(())
    }

    unsafe fn restore_target(&self) {
        if let Some(target) = &self.target {
            self.dc.SetTarget(target);
        }
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
        self.ink = None;
        self.fading = None;
        self.fading_snapshot = false;
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
        self.dc.FillEllipse(
            &ellipse(left + pad + dot, top + height / 2.0, dot),
            &self.brush,
        );
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

impl Drop for Graphics {
    fn drop(&mut self) {
        if !self.waitable.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.waitable);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::{
        IDXGIAdapter3, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    fn history_app() -> App {
        let mut app = App::new(HWND::default(), None, (0, 0));
        for colour in [0xff0000, 0x00ff00, 0x0000ff] {
            let mut look = settings::Settings::default().look;
            look.ink = true;
            look.color = colour;
            app.strokes.push(Stroke {
                points: vec![Point {
                    x: 10.0,
                    y: 20.0,
                    at: Instant::now(),
                }],
                look,
                scale: 1.5,
                baked: true,
            });
        }
        app
    }

    #[test]
    fn cursor_cache_keeps_only_the_current_look_at_all_dpis() {
        let mut cursors = vec![
            ((0xff0000, true, 96), 1),
            ((0xff0000, true, 144), 2),
            ((0x0000ff, true, 96), 3),
            ((0xff0000, false, 96), 4),
        ];
        let mut released = Vec::new();
        retain_cursor_look(&mut cursors, (0xff0000, true, 192), |cursor| {
            released.push(cursor)
        });
        assert_eq!(
            cursors,
            [((0xff0000, true, 96), 1), ((0xff0000, true, 144), 2)]
        );
        assert_eq!(released, [3, 4]);
        retain_cursor_look(&mut cursors, (0xff0000, true, 96), |cursor| {
            released.push(cursor)
        });
        assert_eq!(released, [3, 4]);
        cursors.push(((0xff0000, false, 96), 5));
        retain_cursor_look(&mut cursors, (0xff0000, false, 96), |cursor| {
            released.push(cursor)
        });
        assert_eq!(cursors, [((0xff0000, false, 96), 5)]);
        assert_eq!(released, [3, 4, 1, 2]);
    }

    #[test]
    fn skipped_render_keeps_an_acquired_slot() {
        let mut app = history_app();
        app.frame_slot = true;
        app.render();
        assert!(app.frame_slot);
        app.trimmed = true;
        app.render();
        app.retry_frame();
        assert!(app.frame_slot);
    }

    #[test]
    fn retry_frame_preserves_strokes_and_schedules_full_repaint() {
        let mut app = history_app();
        app.presented = true;
        app.back = 1;
        app.ink_rebuild = false;
        app.fade_rebuild = false;
        app.dirty = false;
        app.previous_dynamic = DirtyRects::whole((100, 100));
        app.retry_frame();
        assert!(!app.presented);
        assert_eq!(app.back, 0);
        assert!(app.previous_dynamic.as_slice().is_empty());
        assert!(app.ink_rebuild && app.fade_rebuild && app.dirty);
        assert_eq!(app.strokes.len(), 3);
        assert_eq!(app.strokes[0].look.color, 0xff0000);
    }

    #[test]
    fn undo_redo_and_clear_restore_stroke_looks() {
        let mut app = history_app();
        app.undo();
        assert_eq!(app.strokes.len(), 2);
        assert!(app.fading.is_none());
        app.redo();
        assert_eq!(app.strokes.len(), 3);
        assert_eq!(app.strokes[2].look.color, 0x0000ff);
        assert_eq!(app.strokes[2].scale, 1.5);
        assert!(!app.strokes[2].baked);
        app.clear();
        assert!(app.strokes.is_empty());
        assert_eq!(app.fading.as_ref().unwrap().strokes.len(), 3);
        app.undo();
        assert_eq!(app.strokes.len(), 3);
        assert!(app.fading.is_none());
        app.undo();
        assert_eq!(app.strokes.len(), 2);
        app.redo();
        assert_eq!(app.strokes.len(), 3);
        app.redo();
        assert!(app.strokes.is_empty());
        app.undo();
        assert_eq!(app.strokes.len(), 3);
    }

    #[test]
    fn keep_toggle_fades_only_kept_strokes() {
        let mut app = history_app();
        app.strokes[1].look.ink = false;
        app.strokes[1].baked = false;
        app.drawing = true;
        app.remove_kept_strokes();
        assert!(!app.drawing);
        assert_eq!(app.strokes.len(), 1);
        assert_eq!(app.strokes[0].look.color, 0x00ff00);
        assert_eq!(app.fading.as_ref().unwrap().strokes.len(), 2);
    }

    #[test]
    fn dirty_rectangles_merge_neighbours() {
        let mut rects = DirtyRects::default();
        rects.add(RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        });
        rects.add(RECT {
            left: 10,
            top: 10,
            right: 30,
            bottom: 30,
        });
        assert_eq!(rects.count, 1);
        assert_eq!(
            rect_bounds(rects.rectangles[0]),
            Bounds {
                left: 0.0,
                top: 0.0,
                right: 30.0,
                bottom: 30.0
            }
        );
        rects.add(RECT {
            left: 33,
            top: 0,
            right: 40,
            bottom: 20,
        });
        assert_eq!(rects.count, 1);
        rects.add(RECT {
            left: 200,
            top: 800,
            right: 250,
            bottom: 830,
        });
        assert_eq!(rects.count, 2);
    }

    #[test]
    fn dirty_rectangles_union_after_eight() {
        let mut rects = DirtyRects::default();
        for index in 0..9 {
            rects.add(RECT {
                left: index * 100,
                top: 0,
                right: index * 100 + 10,
                bottom: 10,
            });
        }
        assert_eq!(rects.count, 1);
        assert_eq!(rects.rectangles[0].left, 0);
        assert_eq!(rects.rectangles[0].right, 810);
    }

    #[test]
    fn empty_dirty_rectangles_stay_empty() {
        let mut rects = DirtyRects::default();
        rects.add(RECT::default());
        rects.extend(&DirtyRects::default());
        assert!(rects.as_slice().is_empty());
    }

    #[test]
    fn stale_damage_reaches_both_buffers_without_including_static_ink() {
        let top = RECT {
            left: 10,
            top: 10,
            right: 100,
            bottom: 40,
        };
        let bottom = RECT {
            left: 10,
            top: 900,
            right: 100,
            bottom: 940,
        };
        let mut buffers = [DirtyRects::default(); 2];
        for buffer in &mut buffers {
            buffer.add(top);
            buffer.add(bottom);
        }
        buffers[0] = DirtyRects::default();
        buffers[1].add(RECT {
            left: 20,
            top: 10,
            right: 120,
            bottom: 40,
        });
        assert_eq!(buffers[1].count, 2);
        assert!(buffers[1]
            .as_slice()
            .iter()
            .all(|rect| rect.bottom - rect.top < 100));
    }

    #[test]
    fn mouse_coordinates_unwrap() {
        assert_eq!(unwrap_coordinate(65535), -1);
        assert_eq!(unwrap_coordinate(32768), -32768);
        assert_eq!(unwrap_coordinate(32767), 32767);
        assert_eq!(unwrap_coordinate(-50), -50);
    }

    #[test]
    fn mouse_history_matches_time_and_position() {
        let sample = |x, time| MouseSample { x, y: -100, time };
        let samples = [sample(8, 12), sample(7, 12), sample(8, 11), sample(5, 10)];
        assert_eq!(history_count(&samples, Some(sample(8, 11))), 2);
        assert_eq!(history_count(&samples, Some(sample(7, 12))), 1);
        assert_eq!(history_count(&samples, Some(sample(9, 11))), 3);
        assert_eq!(history_count(&samples, Some(sample(9, 20))), 0);
        assert_eq!(history_count(&samples, None), 4);
        assert_eq!(history_count(&[], Some(sample(1, 1))), 0);
        let wrapped = [
            sample(3, 1),
            sample(2, 0),
            sample(1, u32::MAX),
            sample(0, u32::MAX - 1),
        ];
        assert_eq!(history_count(&wrapped, Some(sample(1, u32::MAX))), 2);
        let chronological: Vec<_> = wrapped[..2].iter().rev().map(|point| point.x).collect();
        assert_eq!(chronological, [2, 3]);
    }

    #[test]
    fn mouse_timestamps_wrap_and_never_run_ahead() {
        let now = Instant::now();
        assert_eq!(
            now.duration_since(sample_instant(u32::MAX - 9, 10, now)),
            Duration::from_millis(20)
        );
        assert_eq!(sample_instant(11, 10, now), now);
    }

    fn benchmark_stroke(index: usize, now: Instant) -> Stroke {
        let mut look = settings::Settings::default().look;
        look.ink = true;
        let points = (0..120)
            .map(|point| {
                let t = point as f32 / 119.0;
                Point {
                    x: (index % 15) as f32 * 170.0 + 16.0 + t * 135.0,
                    y: (index / 15) as f32 * 144.0 + 70.0 + (t * 12.0).sin() * 40.0,
                    at: now,
                }
            })
            .collect();
        Stroke {
            points,
            look,
            scale: 1.0,
            baked: true,
        }
    }

    fn print_timings(name: &str, timings: &mut [f64], pixels: u64) {
        let mean = timings.iter().sum::<f64>() / timings.len() as f64;
        timings.sort_by(f64::total_cmp);
        let p95 = timings[(timings.len() as f64 * 0.95).ceil() as usize - 1];
        println!("BENCH {name}_mean_cpu={mean:.4} ms");
        println!("BENCH {name}_p95_cpu={p95:.4} ms");
        println!(
            "BENCH {name}_mean_presented_pixels={:.0} pixels",
            pixels as f64 / timings.len() as f64
        );
    }

    fn rectangle_pixels(rectangle: RECT) -> u64 {
        (rectangle.right - rectangle.left) as u64 * (rectangle.bottom - rectangle.top) as u64
    }

    unsafe fn benchmark_drawing() -> Result<()> {
        let size = (2560, 1440);
        let dxgi = graphics_device()?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = factory
            .CreateDevice(&dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let bitmap = |options| {
            dc.CreateBitmap(
                D2D_SIZE_U {
                    width: size.0,
                    height: size.1,
                },
                None,
                0,
                &bitmap_properties(options),
            )
        };
        let target = bitmap(D2D1_BITMAP_OPTIONS_TARGET)?;
        let ink = bitmap(D2D1_BITMAP_OPTIONS_TARGET)?;
        let brush = dc.CreateSolidColorBrush(&rgba(0xff2a2a, 1.0), None)?;
        let now = Instant::now();
        let mut strokes: Vec<_> = (0..150).map(|index| benchmark_stroke(index, now)).collect();
        let mut frames: Vec<_> = (1..=120)
            .map(|count| {
                let mut active = benchmark_stroke(0, now);
                active.points.truncate(count);
                active.baked = false;
                active
            })
            .collect();
        let mut scratch = Scratch::default();
        let mut ink_bounds = None;
        dc.SetTarget(&ink);
        dc.BeginDraw();
        dc.Clear(Some(&D2D1_COLOR_F::default()));
        for stroke in &mut strokes {
            ink_bounds = union(
                ink_bounds,
                stroke::draw(&factory, &dc, &brush, stroke, now, &mut scratch)?,
            );
        }
        dc.EndDraw(None, None)?;
        let layer = InkLayer {
            bitmap: ink,
            bounds: ink_bounds,
        };
        dc.SetTarget(&target);
        dc.BeginDraw();
        dc.Clear(Some(&D2D1_COLOR_F::default()));
        draw_layer(&dc, &layer, full_bounds(size.0, size.1), 1.0);
        dc.EndDraw(None, None)?;

        let mut old_times = [0.0; 120];
        let mut old_pixels = 0;
        for (index, active) in frames.iter_mut().enumerate() {
            let start = Instant::now();
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let mut bounds = None;
            for stroke in &mut strokes {
                bounds = union(
                    bounds,
                    stroke::draw(&factory, &dc, &brush, stroke, now, &mut scratch)?,
                );
            }
            bounds = union(
                bounds,
                stroke::draw(&factory, &dc, &brush, active, now, &mut scratch)?,
            );
            dc.EndDraw(None, None)?;
            old_times[index] = start.elapsed().as_secs_f64() * 1000.0;
            if let Some(bounds) = bounds {
                old_pixels += rectangle_pixels(pixel_rect(bounds, size, 2.0));
            }
        }

        let mut new_times = [0.0; 120];
        let mut new_pixels = 0;
        let mut buffers = [DirtyRects::default(); 2];
        let mut previous = DirtyRects::default();
        for (index, active) in frames.iter_mut().enumerate() {
            let start = Instant::now();
            let mut dynamic = DirtyRects::default();
            if let Some(bounds) = active.bounds() {
                dynamic.add(pixel_rect(bounds, size, 0.0));
            }
            for buffer in &mut buffers {
                buffer.extend(&previous);
                buffer.extend(&dynamic);
            }
            let repaint = buffers[index % 2];
            dc.BeginDraw();
            for &rectangle in repaint.as_slice() {
                let bounds = rect_bounds(rectangle);
                dc.PushAxisAlignedClip(&rect_f(bounds), D2D1_ANTIALIAS_MODE_ALIASED);
                dc.Clear(Some(&D2D1_COLOR_F::default()));
                draw_layer(&dc, &layer, bounds, 1.0);
                stroke::draw(&factory, &dc, &brush, active, now, &mut scratch)?;
                dc.PopAxisAlignedClip();
                new_pixels += rectangle_pixels(rectangle);
            }
            dc.EndDraw(None, None)?;
            new_times[index] = start.elapsed().as_secs_f64() * 1000.0;
            previous = dynamic;
            buffers[index % 2] = DirtyRects::default();
        }
        print_timings("old", &mut old_times, old_pixels);
        print_timings("new", &mut new_times, new_pixels);
        Ok(())
    }

    unsafe fn finish_gpu(dxgi: &IDXGIDevice) -> Result<()> {
        let device: IDXGIDevice3 = dxgi.cast()?;
        let event = CreateEventW(None, false, false, None)?;
        let result = device.EnqueueSetEvent(event).and_then(|()| {
            if WaitForSingleObject(event, 5000) == WAIT_OBJECT_0 {
                Ok(())
            } else {
                Err(windows::core::Error::from(E_FAIL))
            }
        });
        let _ = CloseHandle(event);
        result
    }

    unsafe fn benchmark_memory() -> Result<()> {
        let dxgi = graphics_device()?;
        let adapter: IDXGIAdapter3 = dxgi.GetAdapter()?.cast()?;
        let usage = || -> Result<u64> {
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                .inspect_err(|error| eprintln!("QueryVideoMemoryInfo: {error}"))?;
            Ok(info.CurrentUsage)
        };
        finish_gpu(&dxgi)?;
        let before = usage()?;
        let (_, _, width, height) = virtual_screen();
        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let chain = factory
            .CreateSwapChainForComposition(
                &dxgi,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: width as u32,
                    Height: height as u32,
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
                    ..Default::default()
                },
                None,
            )
            .inspect_err(|error| {
                eprintln!("CreateSwapChainForComposition ({width} x {height}): {error}")
            })?;
        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = d2d
            .CreateDevice(&dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        {
            let surface: IDXGISurface = chain
                .GetBuffer(0)
                .inspect_err(|error| eprintln!("GetBuffer: {error}"))?;
            let bitmap = dc
                .CreateBitmapFromDxgiSurface(
                    &surface,
                    Some(&bitmap_properties(
                        D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                    )),
                )
                .inspect_err(|error| eprintln!("CreateBitmapFromDxgiSurface: {error}"))?;
            dc.SetTarget(&bitmap);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.EndDraw(None, None)
                .inspect_err(|error| eprintln!("EndDraw: {error}"))?;
        }
        dc.SetTarget(None);
        finish_gpu(&dxgi)?;
        let created = usage()?;
        chain
            .ResizeBuffers(2, 8, 8, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
            .inspect_err(|error| eprintln!("ResizeBuffers: {error}"))?;
        let device: IDXGIDevice3 = dxgi.cast()?;
        device.Trim();
        finish_gpu(&dxgi)?;
        let trimmed = usage()?;
        println!("BENCH memory_before={:.3} MB", before as f64 / 1_000_000.0);
        println!(
            "BENCH memory_created={:.3} MB",
            created as f64 / 1_000_000.0
        );
        println!(
            "BENCH memory_trimmed={:.3} MB",
            trimmed as f64 / 1_000_000.0
        );
        Ok(())
    }

    #[test]
    #[ignore]
    fn bench_overlay() -> Result<()> {
        unsafe {
            benchmark_drawing()?;
            benchmark_memory()
        }
    }
}
