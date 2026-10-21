//! Screen-wide laser pointer, driven by user-configurable shortcuts (Alt+L toggles it by default).
//! While on: left-drag draws (hold Alt for a horizontal line, Shift+Alt for a vertical one);
//! right, middle and side clicks and the scroll wheels pass through to the app underneath.
//! A tray icon opens the settings panel.
#![windows_subsystem = "windows"]

#[cfg(test)]
mod bench;
mod constrain;
mod cursors;
mod games;
mod graphics;
mod panel;
mod region;
mod settings;
mod shapes;
#[cfg(test)]
mod snapshot;
mod stroke;
mod ui;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, Result, BOOL};
use windows::Win32::Foundation::{
    GetLastError, COLORREF, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT,
    WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ELLIPSE};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{CreateMutexW, INFINITE};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetMouseMovePointsEx, RegisterHotKey, ReleaseCapture, SendInput, SetCapture,
    UnregisterHotKey, GMMP_USE_DISPLAY_POINTS, HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD,
    INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOD_NOREPEAT, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MOUSEMOVEPOINT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyCursor, DispatchMessageW, GetCursorPos,
    GetMessageTime, GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadCursorW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PostQuitMessage, RegisterClassW, SetCursor, SetCursorPos,
    SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    SystemParametersInfoW, TranslateMessage, GWL_EXSTYLE, HCURSOR, HICON, HWND_TOPMOST,
    IDC_ARROW, IDC_CROSS, LWA_ALPHA, MA_NOACTIVATE, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT,
    SC_CLOSE, SC_KEYMENU, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SPI_GETCLIENTAREAANIMATION, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_CAPTURECHANGED, WM_CHAR, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_KEYDOWN,
    WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SYSCHAR, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
    WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_numerics::Vector2;

use constrain::{Change, Lock};
use graphics::{device_lost, Graphics, Toast};
use region::Region;
use shapes::Correction;
use settings::{Binding, Command, Slot, COMMANDS, MAX_PRESETS};
use stroke::{rgba, Bounds, Point, Scratch, Stroke, MIN_STEP};

// Hotkey ids: commands are 1..=9, presets start here.
const HOTKEY_PRESET: i32 = 100;
const HOTKEY_PROBE: i32 = 99;

const TIMER_PASSTHROUGH: usize = 1;
const TIMER_RECOVER: usize = 2;
const TIMER_TRIM: usize = 3;
const TIMER_CURSOR: usize = 4;
const TIMER_GAME: usize = 5;
/// How soon after taking the foreground to check the game didn't minimise itself.
const GAME_CHECK_MS: u32 = 300;
const PASSTHROUGH_MS: u32 = 80;
const RECOVER_MS: u32 = 1000;
const TRIM_MS: u32 = 3000;
const CURSOR_FRAME_MS: u32 = 16;

// wParam flag on WM_MOUSEWHEEL.
const MK_CONTROL: usize = 0x8;
// An unassigned virtual key: tapping it while Alt is held stops the Alt release from opening the
// foreground app's menu or switching keyboard layouts.
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

const RIPPLE_SECS: f32 = 0.45;
const MORPH_SECS: f32 = 0.1;
const TOAST_SECS: f32 = 1.4;
const CLEAR_SECS: f32 = 0.22;

static LASER_ON: AtomicBool = AtomicBool::new(false);
/// Kept strokes are on screen, so the clear shortcut has something to do.
static INK_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Bit per command (low bits) and per preset (from bit 16) whose shortcut another app holds.
static HOTKEY_FAILED: AtomicU32 = AtomicU32::new(0);
static CURSOR: AtomicIsize = AtomicIsize::new(0);
/// The cursor last handed to SetCursor. It may still be on screen, even with the laser off, so it
/// is never destroyed until something replaces it.
static INSTALLED: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct Ripple {
    x: f32,
    y: f32,
    scale: f32,
    at: Instant,
}

/// A finished stroke settling onto the shape it was recognised as.
struct Morph {
    /// For each point of the shape, the point of the drawn path it slides from.
    from: Vec<(f32, f32)>,
    to: Correction,
    at: Instant,
}

/// A stroke on screen. Kept strokes are baked into the ink layer once finished.
struct Drawn {
    id: u64,
    stroke: Stroke,
    baked: bool,
    morph: Option<Morph>,
}

impl Drawn {
    fn translate(&mut self, dx: f32, dy: f32) {
        self.stroke.translate(dx, dy);
        if let Some(morph) = &mut self.morph {
            for point in morph.from.iter_mut().chain(morph.to.parts.iter_mut().flatten()) {
                point.0 += dx;
                point.1 += dy;
            }
        }
    }

    /// Moves a snapping stroke along its morph; returns whether it is still moving.
    fn advance_morph(&mut self, now: Instant) -> bool {
        let Some(morph) = &self.morph else {
            return false;
        };
        let k = (now.duration_since(morph.at).as_secs_f32() / MORPH_SECS).min(1.0);
        let t = ease_out(k);
        let targets = morph.to.parts.iter().flatten();
        for ((point, &(fx, fy)), &(tx, ty)) in self.stroke.points.iter_mut().zip(&morph.from).zip(targets) {
            point.x = fx + (tx - fx) * t;
            point.y = fy + (ty - fy) * t;
        }
        if k >= 1.0 {
            self.settle();
        }
        k < 1.0
    }

    /// Jumps a snapping stroke to its final shape, before it is put aside for undo or redo.
    fn settle(&mut self) {
        if let Some(morph) = self.morph.take() {
            set_parts(&mut self.stroke, &shapes::lean(&morph.to.parts), morph.at);
        }
    }
}

/// Replaces a stroke's points with `parts`, all stamped `at` so a shape fades as one.
fn set_parts(stroke: &mut Stroke, parts: &[Vec<(f32, f32)>], at: Instant) {
    stroke.points.clear();
    stroke.breaks.clear();
    for part in parts {
        if !stroke.points.is_empty() {
            stroke.breaks.push(stroke.points.len());
        }
        stroke
            .points
            .extend(part.iter().map(|&(x, y)| Point { x, y, at }));
    }
}

/// What undo walks back through.
enum Edit {
    Added(u64),
    Cleared(Vec<Drawn>),
}

enum Redo {
    Stroke(Drawn),
    /// Clears exactly the strokes the undone clear had removed.
    Clear(Vec<u64>),
}

/// The last mouse sample taken into the stroke, in screen coordinates, so the history Windows
/// keeps can be read back without repeating it.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
struct Sample {
    x: i32,
    y: i32,
    time: u32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct CursorKey {
    style: usize,
    color: u32,
    dpi: u32,
    /// Quantised zoom and rotation, so animation frames are rendered once and reused.
    step: u8,
    turn: i8,
}

const ZOOM_MIN: f32 = 0.3;
const ZOOM_STEP: f32 = 0.05;
const TURN_STEP: f32 = 5.0;

fn zoom_step(zoom: f32) -> u8 {
    ((zoom - ZOOM_MIN) / ZOOM_STEP).round().clamp(0.0, 26.0) as u8
}

fn turn_step(degrees: f32) -> i8 {
    (degrees / TURN_STEP).round().clamp(-18.0, 18.0) as i8
}

/// The cursor grows in when the laser turns on and squeezes while the button is held.
#[derive(Default)]
struct Pointer {
    cursors: Vec<(CursorKey, HCURSOR)>,
    current: Option<CursorKey>,
    ignited: Option<Instant>,
    /// Button state and when it last changed.
    pressed: Option<(bool, Instant)>,
}

/// Puts the laser's current cursor on screen, recording it as in use.
fn install_cursor() {
    let cursor = CURSOR.load(Ordering::Relaxed);
    INSTALLED.store(cursor, Ordering::Relaxed);
    unsafe {
        SetCursor(Some(HCURSOR(cursor as _)));
    }
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k).powi(3)
}

impl Pointer {
    /// The current scale and rotation for a style's motion, and whether they're still changing.
    fn pose(&mut self, motion: cursors::Motion, now: Instant) -> (f32, f32, bool) {
        if !animations_enabled() {
            return (1.0, 0.0, false);
        }
        let mut animating = false;
        let mut ignite = 1.0;
        if let Some(at) = self.ignited {
            ignite = now.duration_since(at).as_secs_f32() / motion.ignite_secs();
            if ignite < 1.0 {
                animating = true;
            } else {
                self.ignited = None;
            }
        }
        let mut press = 0.0;
        if let Some((down, at)) = self.pressed {
            let elapsed = now.duration_since(at).as_secs_f32();
            let k = (elapsed / if down { 0.09 } else { 0.14 }).min(1.0);
            press = if down { ease_out(k) } else { 1.0 - ease_out(k) };
            if k < 1.0 {
                animating = true;
            } else if !down {
                self.pressed = None;
            }
        }
        let (scale, turn) = motion.pose(ignite, press);
        (scale, turn, animating)
    }

    /// Shows a cursor now if the laser is on, and hands it to WM_SETCURSOR for later.
    fn publish(&mut self, cursor: HCURSOR) {
        CURSOR.store(cursor.0 as isize, Ordering::Relaxed);
        if laser_on() {
            install_cursor();
        }
    }

    /// Drops cursors for looks no longer in use, and caps how many are kept. The one most
    /// recently installed survives until something else replaces it.
    fn evict(&mut self, keep: CursorKey) {
        let installed = Some(HCURSOR(INSTALLED.load(Ordering::Relaxed) as _));
        let destroy = |cursor: HCURSOR| unsafe {
            let _ = DestroyCursor(cursor);
        };
        self.cursors.retain(|&(key, cursor)| {
            let same_look = key.style == keep.style && key.color == keep.color;
            if same_look || Some(cursor) == installed {
                return true;
            }
            destroy(cursor);
            false
        });
        // Oldest first, but never the frame in use or the one on screen.
        while self.cursors.len() > 48 {
            let spare = self
                .cursors
                .iter()
                .position(|&(key, cursor)| key != keep && Some(cursor) != installed);
            let Some(index) = spare else { break };
            destroy(self.cursors.remove(index).1);
        }
    }
}

fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    enabled.as_bool()
}

struct App {
    hwnd: HWND,
    /// None while recovering from a lost GPU.
    gfx: Option<Graphics>,
    strokes: Vec<Drawn>,
    next_id: u64,
    drawing: bool,
    /// Last mouse sample taken into the stroke being drawn.
    sample: Option<Sample>,
    /// Reused for the mouse history read on each move.
    history: Vec<Sample>,
    /// The whole path of the stroke being drawn, kept for shape snapping even as a fading
    /// stroke's tail disappears.
    path: Vec<(f32, f32)>,
    /// The stroke was pinned to a straight line at some point, so it isn't snapped.
    locked: bool,
    lock: Lock,
    anchor: (f32, f32),
    /// The menu-mask key was already tapped during this press of Alt.
    alt_masked: bool,
    undo: Vec<Edit>,
    redo: Vec<Redo>,
    /// Strokes fading out after a clear.
    clearing: Vec<Stroke>,
    cleared_at: Instant,
    ripples: Vec<Ripple>,
    toast: Option<(Toast, Instant)>,
    dirty: bool,
    /// Virtual-screen position of the window, to map screen coordinates into it.
    origin: (i32, i32),
    size: (u32, u32),
    shown: bool,
    /// Short-lived content (strokes in flight, ripples, the toast) drawn into each swap-chain
    /// buffer last time, which that buffer must repaint before it is shown again.
    stale: [Region; 2],
    /// Kept-ink changes each buffer hasn't caught up with yet.
    ink_damage: [Region; 2],
    /// Kept-ink changes since the last present.
    present_damage: Region,
    back: usize,
    /// False until the buffers have been painted whole since they were (re)created.
    presented: bool,
    scratch: Scratch,
    pointer: Pointer,
    /// The game the overlay took the mouse from while the laser is on.
    game: Option<HWND>,
    /// Games that minimised when the laser took the foreground; never taken from again.
    restless_games: Vec<HWND>,
    warned_exclusive: bool,
    foreground_hook: HWINEVENTHOOK,
}

fn main() -> Result<()> {
    unsafe {
        let _single_instance = CreateMutexW(None, false, w!("Local\\LaserPointer"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            ui::show_running_instance();
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
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

        let size = (width as u32, height as u32);
        let gfx = Graphics::new(hwnd, size.0, size.1)?;
        // Stays hidden until there's something to show, so nothing sits over the desktop.
        APP.with(|app| {
            *app.borrow_mut() = Some(App::new(hwnd, gfx, (x, y), size));
        });
        with_app(App::refresh_cursor);
        // Shortcuts first: adding the tray icon can take Explorer a few seconds.
        sync_hotkeys();
        ui::init(instance.into())?;
        if first_run {
            let shortcut = ui::shortcut_name(settings::get().key(Command::Toggle));
            ui::balloon(
                "Laser Pointer is ready",
                &format!("Press {shortcut} to draw. Settings live in this tray icon."),
            );
        }
        run();
        with_app(App::shutdown);
        ui::shutdown();
    }
    Ok(())
}

/// Sleeps in the kernel when nothing is moving, and paces frames off the swap chain's latency
/// waitable while something is, so input is read as late as possible before each frame.
unsafe fn run() {
    let mut msg = MSG::default();
    loop {
        let waitable = with_app(|app| app.needs_frame().then(|| app.waitable()).flatten()).flatten();
        let handles: &[HANDLE] = match &waitable {
            Some(handle) => std::slice::from_ref(handle),
            None => &[],
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
        if waitable.is_some() && woke == WAIT_OBJECT_0 {
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
    with_app(App::refresh_cursor);
    ui::settings_changed();
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
            with_app(|app| app.clear(|_| true));
        }
        Command::ToggleInk => {
            let mut current = settings::get();
            current.look.ink = !current.look.ink;
            settings::set(current);
            settings::save();
            with_app(|app| {
                // Turning it off lets go of what was kept, as well as what comes next.
                if !current.look.ink {
                    app.clear(|drawn| drawn.stroke.look.ink);
                }
                app.notify(if current.look.ink {
                    "Keep strokes on"
                } else {
                    "Keep strokes off"
                });
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
    with_app(|app| unsafe { app.gfx.as_ref()?.svg_icon(&svg, size, None, 1.0, 0.0).ok() })?
}

/// Renders a cursor style for the settings panel's picker.
pub fn cursor_preview(style: usize, color: u32, size: u32) -> Option<HICON> {
    let style = &cursors::STYLES[style];
    with_app(|app| unsafe {
        app.gfx
            .as_ref()?
            .svg_icon(&style.svg(color), size, None, 1.0, 0.0)
            .ok()
    })?
}

/// The lasso glyph, drawn in the taskbar's own text colour, its dot lit in the laser colour
/// while the laser is on. The taskbar has its own light or dark theme, separate from apps'.
fn tray_svg(on: bool, color: u32) -> String {
    let light = panel::read_dword(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("SystemUsesLightTheme"),
    )
    .is_some_and(|value| value != 0);
    let template = if on {
        include_str!("../assets/brand/tray-on.svg")
    } else {
        include_str!("../assets/brand/tray.svg")
    };
    template
        .replace("INK", if light { "#1b1b1b" } else { "#ffffff" })
        .replace("DOT", &format!("#{color:06x}"))
}

/// Keeps the overlay above borderless games and slide shows that raise themselves.
unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    with_app(|app| {
        if app.shown {
            app.raise();
        }
    });
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
                install_cursor();
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
                // Ctrl as it was at the release, not whenever this message gets handled.
                let freehand = wparam.0 & MK_CONTROL != 0;
                with_app(|app| {
                    app.extend(client_point(lparam));
                    app.finish_stroke(freehand);
                });
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            // Alt-Tab or a dialog took the mouse mid-stroke.
            WM_CAPTURECHANGED => {
                with_app(App::finish);
                LRESULT(0)
            }
            WM_RBUTTONDOWN | WM_MBUTTONDOWN => LRESULT(0),
            WM_XBUTTONDOWN => LRESULT(1),
            // A right-click clicks the app underneath; middle and side buttons pass as themselves.
            WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
                with_app(|app| app.ripple(client_point(lparam)));
                let (down, up, data) = match msg {
                    WM_RBUTTONUP => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
                    WM_MBUTTONUP => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
                    _ => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, ((wparam.0 >> 16) & 0xffff) as u32),
                };
                pass_through(hwnd, &[mouse_input(down, data), mouse_input(up, data)]);
                LRESULT(isize::from(msg == WM_XBUTTONUP))
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
                pass_through(hwnd, &[mouse_input(MOUSEEVENTF_HWHEEL, delta as i32 as u32)]);
                LRESULT(0)
            }
            // Only reachable while the overlay holds the foreground over a game. Alt alone must
            // not enter menu mode, and Alt+F4 must not close the overlay and quit the app.
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR => LRESULT(0),
            WM_SYSCOMMAND
                if matches!((wparam.0 & 0xfff0) as u32, SC_KEYMENU | SC_CLOSE) =>
            {
                LRESULT(0)
            }
            WM_TIMER => {
                match wparam.0 {
                    TIMER_PASSTHROUGH => {
                        let _ = KillTimer(Some(hwnd), TIMER_PASSTHROUGH);
                        if laser_on() {
                            set_click_through(hwnd, false);
                        }
                    }
                    TIMER_RECOVER => {
                        let _ = KillTimer(Some(hwnd), TIMER_RECOVER);
                        // The tray icon is drawn on the new device, outside the app borrow.
                        if with_app(App::recover) == Some(true) {
                            ui::settings_changed();
                        }
                    }
                    TIMER_TRIM => {
                        let _ = KillTimer(Some(hwnd), TIMER_TRIM);
                        with_app(App::trim);
                    }
                    TIMER_GAME => {
                        let _ = KillTimer(Some(hwnd), TIMER_GAME);
                        with_app(App::check_game);
                    }
                    TIMER_CURSOR => {
                        with_app(App::refresh_cursor);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            // The overlay spans every monitor, so it ignores the suggested rectangle.
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

fn monitor_dpi(point: POINT) -> u32 {
    let (mut dpi, mut _y) = (96, 96);
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y);
    }
    dpi
}

fn monitor_scale(point: POINT) -> f32 {
    monitor_dpi(point) as f32 / 96.0
}

/// Windows chooses the cursor afresh only when the mouse moves, or a window appears or goes from
/// under it. Turning click-through on or off does neither, so the old cursor would stay until the
/// mouse next moved. Moving it to where it already is makes Windows ask again.
unsafe fn repick_cursor() {
    let mut at = POINT::default();
    if GetCursorPos(&mut at).is_ok() {
        let _ = SetCursorPos(at.x, at.y);
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

fn key_held(key: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 }
}

fn tap_mask_key() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MASK,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    unsafe {
        SendInput(
            &[key(Default::default()), key(KEYEVENTF_KEYUP)],
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

pub(crate) fn ellipse(x: f32, y: f32, radius: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2 { X: x, Y: y },
        radiusX: radius,
        radiusY: radius,
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

fn intersects(a: Bounds, b: Bounds) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn pixel_rect(bounds: Bounds, (width, height): (u32, u32)) -> Option<RECT> {
    let clamp = |value: f32, max: u32| (value.max(0.0) as i32).min(max as i32);
    let rect = RECT {
        left: clamp(bounds.left.floor(), width),
        top: clamp(bounds.top.floor(), height),
        right: clamp(bounds.right.ceil(), width),
        bottom: clamp(bounds.bottom.ceil(), height),
    };
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}

fn ripple_bounds(ripple: &Ripple) -> Bounds {
    Bounds::around(ripple.x, ripple.y, 40.0 * ripple.scale)
}

/// Converts a time from the mouse's history into an Instant, clamped so a stale stamp can't
/// make a point look older than the stroke it belongs to.
fn sample_instant(time: u32, ticks_now: u32, now: Instant) -> Instant {
    let age = ticks_now.wrapping_sub(time).min(1000);
    now.checked_sub(Duration::from_millis(u64::from(age)))
        .unwrap_or(now)
}

/// Mouse history coordinates are 16-bit; monitors left of or above the primary wrap around.
fn unwrap_coordinate(value: i32) -> i32 {
    if value > 32767 {
        value - 65536
    } else {
        value
    }
}

impl App {
    fn new(hwnd: HWND, gfx: Graphics, origin: (i32, i32), size: (u32, u32)) -> Self {
        let foreground_hook = games::install_foreground_hook(on_foreground);
        Self {
            hwnd,
            gfx: Some(gfx),
            strokes: Vec::new(),
            next_id: 0,
            drawing: false,
            sample: None,
            history: Vec::with_capacity(65),
            path: Vec::new(),
            locked: false,
            lock: Lock::None,
            anchor: (0.0, 0.0),
            alt_masked: false,
            undo: Vec::new(),
            redo: Vec::new(),
            clearing: Vec::new(),
            cleared_at: Instant::now(),
            ripples: Vec::new(),
            toast: None,
            dirty: false,
            origin,
            size,
            shown: false,
            stale: Default::default(),
            ink_damage: Default::default(),
            present_damage: Region::default(),
            back: 0,
            presented: false,
            scratch: Scratch::default(),
            pointer: Pointer::default(),
            game: None,
            restless_games: Vec::new(),
            warned_exclusive: false,
            foreground_hook,
        }
    }

    fn shutdown(&mut self) {
        games::remove_hook(self.foreground_hook);
        // Put a stock cursor up first: a cursor in use mustn't be destroyed.
        if let Ok(arrow) = unsafe { LoadCursorW(None, IDC_ARROW) } {
            unsafe {
                SetCursor(Some(arrow));
            }
            INSTALLED.store(0, Ordering::Relaxed);
        }
        self.pointer.evict(CursorKey {
            style: usize::MAX,
            color: 0,
            dpi: 0,
            step: 0,
            turn: 0,
        });
    }

    fn waitable(&self) -> Option<HANDLE> {
        self.gfx.as_ref().map(Graphics::waitable)
    }

    fn full(&self) -> Bounds {
        Bounds {
            left: 0.0,
            top: 0.0,
            right: self.size.0 as f32,
            bottom: self.size.1 as f32,
        }
    }

    fn needs_frame(&self) -> bool {
        self.shown && (self.dirty || self.animating())
    }

    /// Something on screen changes by itself, without new input.
    fn animating(&self) -> bool {
        // Expired strokes still count until a frame removes them and erases what they left.
        self.strokes.iter().any(|drawn| {
            drawn.morph.is_some() || (!drawn.stroke.look.ink && !drawn.stroke.points.is_empty())
        })
            || !self.ripples.is_empty()
            || self.toast.is_some()
            || !self.clearing.is_empty()
    }

    /// Anything left to show once the laser is off: kept strokes, a fading tail, a notice.
    fn has_content(&self) -> bool {
        !self.strokes.is_empty()
            || !self.ripples.is_empty()
            || self.toast.is_some()
            || !self.clearing.is_empty()
    }

    fn update_ink_visible(&self) {
        let visible = self.strokes.iter().any(|drawn| drawn.stroke.look.ink);
        if INK_VISIBLE.swap(visible, Ordering::Relaxed) != visible {
            sync_hotkeys();
        }
    }

    /// Marks an area whose kept ink changed, for both buffers and the next present.
    fn damage_ink(&mut self, bounds: Bounds) {
        for region in &mut self.ink_damage {
            region.add(bounds);
        }
        self.present_damage.add(bounds);
        self.dirty = true;
    }

    /// Throws the ink layer away and bakes every kept stroke again on the next frame.
    fn rebake(&mut self) {
        if let Some(gfx) = &mut self.gfx {
            gfx.drop_ink();
        }
        for drawn in &mut self.strokes {
            drawn.baked = false;
        }
        let full = self.full();
        self.damage_ink(full);
    }

    fn raise(&self) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }

    fn set_on(&mut self, on: bool) {
        LASER_ON.store(on, Ordering::Relaxed);
        self.finish();
        self.ripples.clear();
        self.toast = None;
        self.dirty = true;
        unsafe {
            set_click_through(self.hwnd, !on);
            if on {
                self.notify("Laser on");
                self.show();
                // Another always-on-top window (a slide show, say) may have been raised since.
                self.raise();
                self.take_mouse_from_game();
                self.pointer.ignited = Some(Instant::now());
                self.refresh_cursor();
                install_cursor();
            } else {
                self.notify("Laser off");
                let _ = ReleaseCapture();
                if let Some(game) = self.game.take() {
                    games::give_back(self.hwnd, game);
                }
                if !self.has_content() {
                    self.hide();
                }
                // The laser's cursor goes now, not when the mouse next moves.
                if let Ok(arrow) = LoadCursorW(None, IDC_ARROW) {
                    SetCursor(Some(arrow));
                    INSTALLED.store(0, Ordering::Relaxed);
                }
            }
            repick_cursor();
        }
        self.update_ink_visible();
    }

    fn warn_fullscreen(&mut self) {
        if !self.warned_exclusive {
            self.warned_exclusive = true;
            ui::balloon(
                "Use Fullscreen Windowed to draw over games",
                "This game is in exclusive fullscreen, which nothing can draw over and which \
                 leaves fullscreen if it loses focus. Switch it to Fullscreen Windowed \
                 (borderless) in its video settings.",
            );
        }
    }

    /// Never takes the foreground from a game in exclusive fullscreen: it would drop out of
    /// fullscreen, as if Alt+Tabbed.
    fn take_mouse_from_game(&mut self) {
        if games::exclusive_fullscreen() {
            self.warn_fullscreen();
            return;
        }
        if !settings::get().game_focus {
            return;
        }
        let (x, y, width, height) = unsafe { virtual_screen() };
        let screen = RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        let Some(game) = games::mouse_capturing_window(self.hwnd, screen) else {
            return;
        };
        if self.restless_games.contains(&game) {
            return;
        }
        if games::take_focus(self.hwnd) {
            self.game = Some(game);
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_GAME, GAME_CHECK_MS, None);
            }
        }
    }

    /// Some fullscreen games minimise when they lose focus anyway: put such a game straight
    /// back, leave its focus alone from now on, and say how to draw over it.
    fn check_game(&mut self) {
        let Some(game) = self.game else { return };
        if games::restore_if_minimised(self.hwnd, game) {
            self.game = None;
            self.restless_games.push(game);
            self.warn_fullscreen();
        }
    }

    /// Paints before showing, so an old frame never flashes up.
    fn show(&mut self) {
        if self.shown {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TRIM);
        }
        if let Some(gfx) = &mut self.gfx {
            if gfx.trimmed {
                if unsafe { gfx.untrim() }.is_err() {
                    self.lose_device();
                }
                self.presented = false;
            }
        }
        self.render();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.shown = true;
    }

    /// Hiding starts a timer that hands the screen-sized buffers back if nothing shows up soon.
    fn hide(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            SetTimer(Some(self.hwnd), TIMER_TRIM, TRIM_MS, None);
        }
        self.shown = false;
    }

    fn trim(&mut self) {
        if self.shown || self.has_content() {
            return;
        }
        if let Some(gfx) = &mut self.gfx {
            if unsafe { gfx.trim() }.is_err() {
                self.lose_device();
            }
        }
        self.presented = false;
    }

    /// The GPU was reset or removed: rebuild everything from scratch. The strokes are only
    /// geometry, so nothing the user drew is lost.
    fn lose_device(&mut self) {
        self.gfx = None;
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_RECOVER, RECOVER_MS / 4, None);
        }
    }

    /// Returns whether the device is back.
    fn recover(&mut self) -> bool {
        let (width, height) = self.size;
        match unsafe { Graphics::new(self.hwnd, width, height) } {
            Ok(gfx) => {
                self.gfx = Some(gfx);
                self.presented = false;
                self.stale = Default::default();
                self.pointer.current = None;
                self.rebake();
                self.refresh_cursor();
                true
            }
            Err(_) => {
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_RECOVER, RECOVER_MS, None);
                }
                false
            }
        }
    }

    /// A pill near the bottom of the monitor the cursor is on, if the user wants them.
    fn notify(&mut self, text: &str) {
        if !settings::get().indicator {
            return;
        }
        let mut cursor = POINT::default();
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            let _ = GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info);
        }
        let scale = monitor_scale(cursor);
        let work = info.rcWork;
        let centre = ((work.left + work.right) / 2 - self.origin.0) as f32;
        let bottom = (work.bottom - self.origin.1) as f32 - 72.0 * scale;
        let Some(gfx) = &self.gfx else { return };
        self.toast = unsafe { gfx.make_toast(text, centre, bottom, scale) }.map(|toast| (toast, Instant::now()));
        self.dirty = true;
        self.show();
    }

    fn screen_point(&self, (x, y): (f32, f32)) -> POINT {
        POINT {
            x: x as i32 + self.origin.0,
            y: y as i32 + self.origin.1,
        }
    }

    fn lock_held(&mut self) -> Lock {
        let alt = key_held(VK_MENU);
        if !alt {
            self.alt_masked = false;
        }
        Lock::from_modifiers(alt, key_held(VK_SHIFT))
    }

    /// The first time a line starts during a press of Alt, stops the foreground app from
    /// treating the Alt release as a request for its menu or a keyboard-layout switch.
    fn mask_alt(&mut self) {
        if !self.alt_masked {
            self.alt_masked = true;
            tap_mask_key();
        }
    }

    fn begin(&mut self, point: (f32, f32)) {
        if !laser_on() {
            return;
        }
        self.finish();
        let screen = self.screen_point(point);
        let time = unsafe { GetMessageTime() } as u32;
        let now = Instant::now();
        let at = sample_instant(time, unsafe { GetTickCount() }, now);
        let stroke = Stroke::new(
            Point {
                x: point.0,
                y: point.1,
                at,
            },
            settings::get().look,
            monitor_scale(screen),
        );
        self.next_id += 1;
        self.strokes.push(Drawn {
            id: self.next_id,
            stroke,
            baked: false,
            morph: None,
        });
        self.path.clear();
        self.path.push(point);
        self.undo.push(Edit::Added(self.next_id));
        self.redo.clear();
        self.sample = Some(Sample {
            x: screen.x,
            y: screen.y,
            time,
        });
        // Each gesture masks Alt afresh; a release between strokes may have gone unseen.
        self.alt_masked = false;
        self.lock = self.lock_held();
        self.anchor = point;
        self.locked = self.lock != Lock::None;
        if self.locked {
            self.mask_alt();
        }
        self.drawing = true;
        self.dirty = true;
        self.pointer.pressed = Some((true, now));
        self.refresh_cursor();
        self.update_ink_visible();
    }

    /// Adds the mouse's path since the last message, including the samples Windows coalesced
    /// away, so a fast flick stays round instead of turning into a polygon.
    fn extend(&mut self, point: (f32, f32)) {
        if !self.drawing {
            return;
        }
        let lock = self.lock_held();
        match constrain::change(self.lock, lock) {
            Change::Keep => {}
            Change::Anchor => {
                self.anchor = self
                    .strokes
                    .last()
                    .and_then(|drawn| drawn.stroke.points.last())
                    .map_or(point, |last| (last.x, last.y));
                self.locked = true;
                self.mask_alt();
            }
            Change::Restart => {
                self.finish();
                self.begin(point);
                return;
            }
        }
        self.lock = lock;
        let screen = self.screen_point(point);
        let time = unsafe { GetMessageTime() } as u32;
        let (now, ticks) = (Instant::now(), unsafe { GetTickCount() });
        let current = Sample {
            x: screen.x,
            y: screen.y,
            time,
        };
        self.read_history(current);
        for index in 0..self.history.len() {
            self.append(self.history[index], now, ticks);
        }
        self.sample = Some(current);
    }

    /// Fills `history` with the samples between the last one taken and `current`, oldest first,
    /// ending with `current`.
    fn read_history(&mut self, current: Sample) {
        self.history.clear();
        let query = MOUSEMOVEPOINT {
            x: current.x & 0xffff,
            y: current.y & 0xffff,
            time: current.time,
            dwExtraInfo: 0,
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
        if let Ok(count) = usize::try_from(count) {
            // Newest first: walk back until reaching what was already taken.
            let last = self.sample;
            self.history.extend(
                raw[..count]
                    .iter()
                    .map(|point| Sample {
                        x: unwrap_coordinate(point.x),
                        y: unwrap_coordinate(point.y),
                        time: point.time,
                    })
                    .take_while(|sample| {
                        last.is_none_or(|last| {
                            *sample != last && sample.time.wrapping_sub(last.time) < 0x8000_0000
                        })
                    }),
            );
            self.history.reverse();
        }
        if self.history.last() != Some(&current) {
            self.history.push(current);
        }
    }

    fn append(&mut self, sample: Sample, now: Instant, ticks: u32) {
        let (lock, anchor) = (self.lock, self.anchor);
        let origin = self.origin;
        let Some(drawn) = self.strokes.last_mut() else {
            return;
        };
        let stroke = &mut drawn.stroke;
        let (x, y) = lock.project(
            ((sample.x - origin.0) as f32, (sample.y - origin.1) as f32),
            anchor,
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
        let snappable = stroke.look.snap > 0 && !self.locked;
        stroke.points.push(Point { x, y, at });
        if snappable {
            // Long holds thin out rather than grow without end; shapes don't need every point.
            if self.path.len() >= 4096 {
                let mut keep = false;
                self.path.retain(|_| {
                    keep = !keep;
                    keep
                });
            }
            self.path.push((x, y));
        }
        self.dirty = true;
    }

    fn finish(&mut self) {
        self.finish_stroke(false);
    }

    /// Ends the stroke being drawn, snapping it to a shape unless `freehand`.
    fn finish_stroke(&mut self, freehand: bool) {
        if !self.drawing {
            return;
        }
        self.drawing = false;
        self.sample = None;
        self.lock = Lock::None;
        if !freehand {
            self.snap_finished();
        }
        self.path.clear();
        self.dirty = true;
        self.pointer.pressed = Some((false, Instant::now()));
        self.refresh_cursor();
    }

    /// Pulls the stroke just finished onto the line, arrow or shape it looks like, unless it was
    /// a straight line already.
    fn snap_finished(&mut self) {
        let Some(drawn) = self.strokes.last_mut() else {
            return;
        };
        let snap = drawn.stroke.look.snap;
        if self.locked || snap == 0 {
            return;
        }
        let Some(correction) = shapes::correct(&self.path, drawn.stroke.scale, snap as f32 / 100.0)
        else {
            return;
        };
        let now = Instant::now();
        drawn.stroke.whole = true;
        // Corners stay sharp; the shape is already smooth where it should be.
        drawn.stroke.look.smoothing = 0;
        if !animations_enabled() {
            set_parts(&mut drawn.stroke, &shapes::lean(&correction.parts), now);
            return;
        }
        set_parts(&mut drawn.stroke, &correction.parts, now);
        let from = shapes::correspondence(&self.path, &correction);
        drawn.morph = Some(Morph {
            from,
            to: correction,
            at: now,
        });
        drawn.advance_morph(now);
    }

    fn remove_stroke(&mut self, id: u64) -> Option<Drawn> {
        let index = self.strokes.iter().position(|drawn| drawn.id == id)?;
        let mut drawn = self.strokes.remove(index);
        drawn.settle();
        if drawn.baked {
            self.rebake();
        }
        Some(drawn)
    }

    /// Puts strokes back where they were in drawing order (ids grow as strokes are drawn), so
    /// overlaps come back the way they were. Anything baked above them is baked again.
    fn restore(&mut self, strokes: Vec<Drawn>) {
        let Some(oldest) = strokes.iter().map(|drawn| drawn.id).min() else {
            return;
        };
        self.strokes.extend(strokes);
        self.strokes.sort_by_key(|drawn| drawn.id);
        if self.strokes.iter().any(|drawn| drawn.baked && drawn.id > oldest) {
            self.rebake();
        }
    }

    fn undo(&mut self) {
        if self.drawing {
            return;
        }
        let now = Instant::now();
        while let Some(edit) = self.undo.pop() {
            match edit {
                Edit::Added(id) => {
                    // A fading stroke that has already gone has nothing left to undo.
                    if let Some(drawn) = self.remove_stroke(id) {
                        self.redo.push(Redo::Stroke(drawn));
                        break;
                    }
                }
                Edit::Cleared(list) => {
                    let restored: Vec<Drawn> = list
                        .into_iter()
                        .filter(|drawn| !drawn.stroke.expired(now))
                        .map(|drawn| Drawn {
                            baked: false,
                            ..drawn
                        })
                        .collect();
                    if restored.is_empty() {
                        continue;
                    }
                    self.clearing.clear();
                    self.redo.push(Redo::Clear(restored.iter().map(|drawn| drawn.id).collect()));
                    self.restore(restored);
                    break;
                }
            }
        }
        self.dirty = true;
        self.update_ink_visible();
    }

    fn redo(&mut self) {
        if self.drawing {
            return;
        }
        match self.redo.pop() {
            Some(Redo::Stroke(mut drawn)) => {
                // A fading stroke fades again from now rather than vanishing at once.
                if let Some(last) = drawn.stroke.points.last().map(|point| point.at) {
                    let shift = Instant::now().saturating_duration_since(last);
                    for point in &mut drawn.stroke.points {
                        point.at += shift;
                    }
                }
                drawn.baked = false;
                self.undo.push(Edit::Added(drawn.id));
                self.restore(vec![drawn]);
            }
            Some(Redo::Clear(ids)) => {
                let redo = std::mem::take(&mut self.redo);
                self.clear(|drawn| ids.contains(&drawn.id));
                self.redo = redo;
            }
            None => return,
        }
        self.dirty = true;
        self.update_ink_visible();
    }

    /// Fades out the strokes `which` picks, as one undoable step.
    fn clear(&mut self, which: impl Fn(&Drawn) -> bool) {
        self.finish();
        let (gone, kept): (Vec<Drawn>, Vec<Drawn>) = std::mem::take(&mut self.strokes)
            .into_iter()
            .partition(|drawn| which(drawn));
        self.strokes = kept;
        if gone.is_empty() {
            return;
        }
        let mut gone = gone;
        for drawn in &mut gone {
            drawn.settle();
        }
        if gone.iter().any(|drawn| drawn.baked) {
            self.rebake();
        }
        self.clearing = gone.iter().map(|drawn| drawn.stroke.clone()).collect();
        self.cleared_at = Instant::now();
        self.undo.push(Edit::Cleared(gone));
        self.redo.clear();
        self.dirty = true;
        self.update_ink_visible();
    }

    fn ripple(&mut self, (x, y): (f32, f32)) {
        if settings::get().ripples {
            let scale = monitor_scale(self.screen_point((x, y)));
            self.ripples.push(Ripple {
                x,
                y,
                scale,
                at: Instant::now(),
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
    }

    /// Monitors were added, removed or rearranged. Strokes keep their place on screen.
    fn fit_to_screen(&mut self) {
        let (x, y, width, height) = unsafe { virtual_screen() };
        let (dx, dy) = ((self.origin.0 - x) as f32, (self.origin.1 - y) as f32);
        for drawn in &mut self.strokes {
            drawn.translate(dx, dy);
        }
        // Undo and redo must bring strokes back where they were on screen, too.
        for edit in &mut self.undo {
            if let Edit::Cleared(list) = edit {
                for drawn in list {
                    drawn.translate(dx, dy);
                }
            }
        }
        for redo in &mut self.redo {
            if let Redo::Stroke(drawn) = redo {
                drawn.translate(dx, dy);
            }
        }
        for point in &mut self.path {
            point.0 += dx;
            point.1 += dy;
        }
        self.anchor = (self.anchor.0 + dx, self.anchor.1 + dy);
        for stroke in &mut self.clearing {
            stroke.translate(dx, dy);
        }
        for ripple in &mut self.ripples {
            ripple.x += dx;
            ripple.y += dy;
        }
        self.toast = None;
        self.origin = (x, y);
        self.size = (width as u32, height as u32);
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
        if let Some(gfx) = &mut self.gfx {
            if unsafe { gfx.resize(width as u32, height as u32) }.is_err() {
                self.lose_device();
            }
        }
        self.presented = false;
        self.stale = Default::default();
        self.rebake();
    }

    /// Picks the cursor for the current style, colour, monitor and animation frame, rendering it
    /// the first time it's needed. Keeps a timer running only while the cursor is animating.
    /// Picks the cursor for the current style, colour, monitor and animation frame, rendering it
    /// the first time it's needed. Keeps a timer running only while the cursor is animating.
    fn refresh_cursor(&mut self) {
        let settings = settings::get();
        let motion = cursors::STYLES[settings.cursor].motion;
        let (zoom, turn, animating) = self.pointer.pose(motion, Instant::now());
        let mut position = POINT::default();
        let located = unsafe { GetCursorPos(&mut position) }.is_ok();
        unsafe {
            if animating && located && laser_on() {
                SetTimer(Some(self.hwnd), TIMER_CURSOR, CURSOR_FRAME_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_CURSOR);
            }
        }
        if !located {
            return;
        }
        let key = CursorKey {
            style: settings.cursor,
            color: settings.look.color,
            dpi: monitor_dpi(position),
            step: zoom_step(zoom),
            turn: turn_step(turn),
        };
        if self.pointer.current == Some(key) {
            return;
        }
        let cached = self.pointer.cursors.iter().find(|(cached, _)| *cached == key);
        let cursor = match cached {
            Some((_, cursor)) => *cursor,
            None => match self.render_cursor(key) {
                Some(cursor) => {
                    self.pointer.cursors.push((key, cursor));
                    cursor
                }
                // Without a GPU for now: a shared system crosshair, never cached or destroyed,
                // and the real cursor is tried again next time.
                None => {
                    self.pointer.current = None;
                    if let Ok(cross) = unsafe { LoadCursorW(None, IDC_CROSS) } {
                        self.pointer.publish(cross);
                    }
                    return;
                }
            },
        };
        self.pointer.current = Some(key);
        self.pointer.publish(cursor);
        // Only once the replacement is showing can the old ones go.
        self.pointer.evict(key);
    }
    fn render_cursor(&self, key: CursorKey) -> Option<HCURSOR> {
        let style = &cursors::STYLES[key.style];
        let scale = key.dpi as f32 / 96.0;
        let size = (cursors::SIZE * scale).round() as u32;
        let hotspot = (style.hotspot.0 * scale, style.hotspot.1 * scale);
        let zoom = ZOOM_MIN + f32::from(key.step) * ZOOM_STEP;
        let degrees = f32::from(key.turn) * TURN_STEP;
        let gfx = self.gfx.as_ref()?;
        let icon =
            unsafe { gfx.svg_icon(&style.svg(key.color), size, Some(hotspot), zoom, degrees) }.ok()?;
        Some(HCURSOR(icon.0))
    }
    /// Bakes finished kept strokes into the ink layer, so they cost nothing to keep on screen.
    /// Bakes finished kept strokes into the ink layer, so they cost nothing to keep on screen.
    fn bake_finished(&mut self) {
        let Some(gfx) = &mut self.gfx else { return };
        let last = self.strokes.len().wrapping_sub(1);
        let mut damaged = Region::default();
        let mut lost = false;
        for (index, drawn) in self.strokes.iter_mut().enumerate() {
            let in_progress = self.drawing && index == last;
            if drawn.baked {
                continue;
            }
            // Baking puts a stroke beneath everything still live, so it waits until every stroke
            // drawn before it is baked or gone; the ink keeps the order things were drawn in.
            if !drawn.stroke.look.ink || in_progress || drawn.morph.is_some() {
                break;
            }
            let (scratch, stroke) = (&mut self.scratch, &mut drawn.stroke);
            let baked = unsafe {
                gfx.bake(|gfx| {
                    stroke::draw(&gfx.factory, &gfx.dc, &gfx.brush, stroke, Instant::now(), 1.0, scratch);
                })
            };
            if let Err(error) = baked {
                lost = device_lost(&error);
                break;
            }
            drawn.baked = true;
            if let Some(bounds) = drawn.stroke.bounds() {
                damaged.add(bounds);
            }
        }
        for bounds in damaged.rects() {
            self.damage_ink(bounds);
        }
        if lost {
            self.lose_device();
        }
    }
    fn render(&mut self) {
        let now = Instant::now();
        let color = settings::get().look.color;
        self.ripples
            .retain(|ripple| now.duration_since(ripple.at).as_secs_f32() < RIPPLE_SECS);
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, at)| now.duration_since(*at).as_secs_f32() >= TOAST_SECS)
        {
            self.toast = None;
        }
        if now.duration_since(self.cleared_at).as_secs_f32() >= CLEAR_SECS {
            self.clearing.clear();
        }
        let last = self.strokes.len().wrapping_sub(1);
        let drawing = self.drawing;
        let mut index = 0;
        self.strokes.retain(|drawn| {
            index += 1;
            !drawn.stroke.expired(now) || (drawing && index - 1 == last)
        });
        for drawn in &mut self.strokes {
            drawn.advance_morph(now);
            drawn.stroke.prune(now);
        }
        self.bake_finished();
        let Some(gfx) = &self.gfx else { return };
        if gfx.trimmed {
            return;
        }
        let size = gfx.size();

        // What moves this frame, and where.
        let mut moving = Region::default();
        for drawn in self.strokes.iter().filter(|drawn| !drawn.baked) {
            if let Some(bounds) = drawn.stroke.bounds() {
                moving.add(bounds);
            }
        }
        for stroke in &self.clearing {
            if let Some(bounds) = stroke.bounds() {
                moving.add(bounds);
            }
        }
        for ripple in &self.ripples {
            moving.add(ripple_bounds(ripple));
        }
        if let Some((toast, _)) = &self.toast {
            moving.add(Graphics::toast_bounds(toast));
        }

        // One set of rectangles is both repainted and presented. It covers what this buffer
        // showed two frames ago, what the frame on screen shows, what moves now, and kept ink
        // that changed since either buffer last saw it.
        let back = self.back;
        let mut changed = moving;
        changed.union(&self.stale[back]);
        changed.union(&self.stale[1 - back]);
        changed.union(&self.ink_damage[back]);
        changed.union(&self.present_damage);
        let mut rects = [RECT::default(); 8];
        let mut count = 0;
        if self.presented {
            for bounds in changed.rects() {
                if let Some(rect) = pixel_rect(inflate(bounds, 2.0), size) {
                    rects[count] = rect;
                    count += 1;
                }
            }
            // Every frame-latency wait must be paired with a present, so even an unchanged
            // frame presents one repainted pixel.
            if count == 0 {
                rects[0] = RECT {
                    left: 0,
                    top: 0,
                    right: 1,
                    bottom: 1,
                };
                count = 1;
            }
        } else {
            rects[0] = RECT {
                left: 0,
                top: 0,
                right: size.0 as i32,
                bottom: size.1 as i32,
            };
            count = 1;
        }
        let clear_fade = 1.0 - (now.duration_since(self.cleared_at).as_secs_f32() / CLEAR_SECS).min(1.0);

        let drawn = unsafe {
            gfx.dc.BeginDraw();
            for rect in &rects[..count] {
                let clip = D2D_RECT_F {
                    left: rect.left as f32,
                    top: rect.top as f32,
                    right: rect.right as f32,
                    bottom: rect.bottom as f32,
                };
                let area = Bounds {
                    left: clip.left,
                    top: clip.top,
                    right: clip.right,
                    bottom: clip.bottom,
                };
                gfx.dc.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
                gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
                gfx.draw_ink(clip);
                for drawn in self.strokes.iter_mut().filter(|drawn| !drawn.baked) {
                    let stroke = &mut drawn.stroke;
                    if stroke.bounds().is_some_and(|bounds| intersects(bounds, area)) {
                        stroke::draw(&gfx.factory, &gfx.dc, &gfx.brush, stroke, now, 1.0, &mut self.scratch);
                    }
                }
                for stroke in &mut self.clearing {
                    if stroke.bounds().is_some_and(|bounds| intersects(bounds, area)) {
                        let ease = clear_fade * clear_fade;
                        stroke::draw(&gfx.factory, &gfx.dc, &gfx.brush, stroke, self.cleared_at, ease, &mut self.scratch);
                    }
                }
                for ripple in &self.ripples {
                    if intersects(ripple_bounds(ripple), area) {
                        let k = now.duration_since(ripple.at).as_secs_f32() / RIPPLE_SECS;
                        let radius = (6.0 + 30.0 * (1.0 - (1.0 - k).powi(3))) * ripple.scale;
                        gfx.brush.SetColor(&rgba(color, 0.9 * (1.0 - k).powf(1.5)));
                        gfx.dc.DrawEllipse(
                            &ellipse(ripple.x, ripple.y, radius),
                            &gfx.brush,
                            3.0 * ripple.scale * (1.0 - 0.5 * k),
                            None,
                        );
                    }
                }
                if let Some((toast, at)) = &self.toast {
                    if intersects(Graphics::toast_bounds(toast), area) {
                        let age = now.duration_since(*at).as_secs_f32();
                        let alpha = (age / 0.08).min(1.0).min((TOAST_SECS - age) / 0.3).max(0.0);
                        gfx.draw_toast(toast, color, alpha);
                    }
                }
                gfx.dc.PopAxisAlignedClip();
            }
            gfx.dc.EndDraw(None, None)
        };
        let whole = !self.presented;
        let presented = drawn.and_then(|()| unsafe {
            gfx.present(if whole { &mut [] } else { &mut rects[..count] })
        });

        self.stale[back] = moving;
        self.ink_damage[back].clear();
        self.present_damage.clear();
        self.back ^= 1;
        self.presented = true;
        self.dirty = false;
        if let Err(error) = presented {
            if device_lost(&error) {
                self.lose_device();
            }
        }
        self.update_ink_visible();
        if !laser_on() && !self.has_content() && self.shown {
            self.hide();
        }
    }}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_history_coordinates_unwrap_to_negative_monitors() {
        assert_eq!(unwrap_coordinate(100), 100);
        assert_eq!(unwrap_coordinate(65536 - 1920), -1920);
    }

    #[test]
    fn sample_times_never_run_ahead_or_far_behind() {
        let now = Instant::now();
        assert_eq!(sample_instant(1000, 1000, now), now);
        assert_eq!(now.duration_since(sample_instant(990, 1000, now)), Duration::from_millis(10));
        // A stamp from the future, or wrapped around, is treated as at most a second old.
        assert!(now.duration_since(sample_instant(2000, 1000, now)) <= Duration::from_secs(1));
    }

    #[test]
    fn idle_cursor_zoom_is_exactly_one() {
        assert_eq!(ZOOM_MIN + f32::from(zoom_step(1.0)) * ZOOM_STEP, 1.0);
        assert_eq!(zoom_step(0.0), 0);
        assert_eq!(zoom_step(5.0), 26);
    }

    #[test]
    fn ignition_overshoots_then_settles() {
        use cursors::back_out;
        assert!((back_out(0.0)).abs() < 1e-6);
        assert!((back_out(1.0) - 1.0).abs() < 1e-6);
        assert!((0..20).map(|i| back_out(i as f32 / 20.0)).fold(0.0, f32::max) > 1.0);
    }

    #[test]
    fn resting_turn_is_exactly_zero() {
        assert_eq!(turn_step(0.0), 0);
        assert_eq!(turn_step(45.0), 9);
        assert_eq!(turn_step(-400.0), -18);
    }

    #[test]
    fn dirty_rects_are_clamped_to_the_buffer() {
        let bounds = Bounds {
            left: -10.0,
            top: 5.5,
            right: 50.2,
            bottom: 2000.0,
        };
        let rect = pixel_rect(bounds, (40, 100)).unwrap();
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (0, 5, 40, 100));
        assert!(pixel_rect(Bounds::around(-50.0, -50.0, 5.0), (40, 100)).is_none());
    }
}
