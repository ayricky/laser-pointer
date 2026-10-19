//! Tray icon, its right-click menu, and the window that hosts the settings panel.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, InvalidateRect, MonitorFromPoint, ValidateRect, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForSystem, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyNameTextW, MapVirtualKeyW, ReleaseCapture, SetCapture, TrackMouseEvent, MAPVK_VK_TO_VSC,
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NINF_KEY, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, FindWindowW, GetCursorPos, KillTimer, LoadCursorW,
    LoadIconW, PostMessageW, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    SetTimer, SetWindowPos, ShowWindow, TrackPopupMenu, ASFW_ANY, CS_DROPSHADOW, HICON,
    HWND_TOPMOST, IDC_ARROW, IDI_APPLICATION, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, SM_CXSMICON, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
    TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WA_INACTIVE, WM_ACTIVATE,
    WM_APP, WM_CHAR, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NULL, WM_PAINT,
    WM_SETTINGCHANGE, WM_SYSCHAR, WM_SYSKEYDOWN, WM_THEMECHANGED, WM_TIMER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::panel::{self, Action, Panel};
use crate::settings::{self, Binding, Command};

const TRAY_CLASS: PCWSTR = w!("LaserPointerTray");
const PANEL_CLASS: PCWSTR = w!("LaserPointerFlyout");
const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const TIMER_PREVIEW: usize = 1;
// Not in the bindings' WindowsAndMessaging module.
const WM_MOUSELEAVE: u32 = 0x02a3;

const MENU_TOGGLE: usize = 1;
const MENU_INK: usize = 2;
const MENU_CLEAR: usize = 3;
const MENU_SETTINGS: usize = 4;
const MENU_QUIT: usize = 5;
const MENU_PRESET: usize = 100;

#[derive(Default)]
struct State {
    instance: Cell<HINSTANCE>,
    tray: Cell<HWND>,
    flyout: Cell<HWND>,
    icon: Cell<HICON>,
    closed_at: Cell<Option<Instant>>,
    modal: Cell<bool>,
    tracking: Cell<bool>,
    taskbar_created: Cell<u32>,
    show_message: Cell<u32>,
}

// Cells rather than one RefCell: most of these Win32 calls re-enter a window procedure.
thread_local! {
    static STATE: State = State::default();
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
}

fn state<R>(action: impl FnOnce(&State) -> R) -> R {
    STATE.with(action)
}

fn with_panel<R>(action: impl FnOnce(&mut Panel) -> R) -> Option<R> {
    PANEL.with(|panel| {
        let mut panel = panel.try_borrow_mut().ok()?;
        panel.as_mut().map(action)
    })
}

fn show_message() -> u32 {
    unsafe { RegisterWindowMessageW(w!("LaserPointer.ShowSettings")) }
}

/// Called by a second copy at startup: asks the running one to open its settings instead.
pub fn show_running_instance() {
    unsafe {
        if let Ok(tray) = FindWindowW(TRAY_CLASS, PCWSTR::null()) {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = PostMessageW(Some(tray), show_message(), WPARAM(0), LPARAM(0));
        }
    }
}

pub fn init(instance: HINSTANCE) -> Result<()> {
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(tray_proc),
            hInstance: instance,
            lpszClassName: TRAY_CLASS,
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(panel_proc),
            hInstance: instance,
            lpszClassName: PANEL_CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });
        // Never shown; it owns the tray icon and gives the tray menu a window to be foreground.
        let tray = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            TRAY_CLASS,
            w!("Laser pointer"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?;
        state(|s| {
            s.instance.set(instance);
            s.tray.set(tray);
            s.taskbar_created
                .set(RegisterWindowMessageW(w!("TaskbarCreated")));
            s.show_message.set(show_message());
        });
        update_tray(true);
    }
    Ok(())
}

pub fn shutdown() {
    unsafe {
        let flyout = state(|s| s.flyout.get());
        if !flyout.is_invalid() {
            let _ = DestroyWindow(flyout);
        }
        let _ = Shell_NotifyIconW(NIM_DELETE, &notify_data());
    }
}

pub fn panel_open() -> bool {
    !state(|s| s.flyout.get()).is_invalid()
}

pub fn laser_changed() {
    unsafe {
        update_tray(false);
        let flyout = state(|s| s.flyout.get());
        if !flyout.is_invalid() {
            // Switching on raises the overlay; keep the panel above it so it stays usable.
            let _ = SetWindowPos(
                flyout,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(Some(flyout), None, false);
        }
    }
}

pub fn settings_changed() {
    unsafe {
        update_tray(false);
        let flyout = state(|s| s.flyout.get());
        if !flyout.is_invalid() {
            let _ = InvalidateRect(Some(flyout), None, false);
        }
    }
}

pub fn shortcut_name(binding: Binding) -> String {
    if !binding.is_set() {
        return "Not set".into();
    }
    let mut name = String::new();
    for (flag, label) in [
        (MOD_CONTROL, "Ctrl+"),
        (MOD_ALT, "Alt+"),
        (MOD_SHIFT, "Shift+"),
        (MOD_WIN, "Win+"),
    ] {
        if binding.mods & flag.0 != 0 {
            name.push_str(label);
        }
    }
    // Navigation keys share scan codes with the number pad and need the extended bit.
    let extended = matches!(binding.vk, 0x21..=0x28 | 0x2d | 0x2e | 0x5b | 0x5c | 0x6f | 0x90);
    let mut key = [0u16; 32];
    let length = unsafe {
        let scan = MapVirtualKeyW(binding.vk, MAPVK_VK_TO_VSC);
        let flags = (scan << 16) | if extended { 1 << 24 } else { 0 };
        GetKeyNameTextW(flags as i32, &mut key)
    };
    if length > 0 {
        name.push_str(&String::from_utf16_lossy(&key[..length as usize]));
    } else {
        name.push_str(&format!("Key {}", binding.vk));
    }
    name
}

fn notify_data() -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: state(|s| s.tray.get()),
        uID: TRAY_ID,
        ..Default::default()
    }
}

unsafe fn update_tray(add: bool) {
    let on = crate::laser_on();
    let current = settings::get();
    let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) as u32;
    let icon = crate::tray_icon(on, size)
        .unwrap_or_else(|| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
    let mut data = notify_data();
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    let tip = format!(
        "Laser pointer: {} ({})",
        if on { "on" } else { "off" },
        shortcut_name(current.key(Command::Toggle))
    );
    for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *slot = unit;
    }
    if add {
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
    } else {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
    let old = state(|s| s.icon.replace(icon));
    if !old.is_invalid() && old != icon {
        let _ = DestroyIcon(old);
    }
}

extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_TRAY {
            match (lparam.0 & 0xffff) as u32 {
                NIN_SELECT | NIN_KEYSELECT => toggle_panel(),
                WM_CONTEXTMENU => show_menu(
                    hwnd,
                    (wparam.0 & 0xffff) as i16 as i32,
                    ((wparam.0 >> 16) & 0xffff) as i16 as i32,
                ),
                _ => {}
            }
            return LRESULT(0);
        }
        if msg != 0 && msg == state(|s| s.show_message.get()) {
            let _ = open_panel();
            return LRESULT(0);
        }
        // Explorer restarted: the old icon is gone.
        if msg != 0 && msg == state(|s| s.taskbar_created.get()) {
            update_tray(true);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn with_shortcut(label: &str, binding: Binding) -> HSTRING {
    if binding.is_set() {
        HSTRING::from(format!("{label}\t{}", shortcut_name(binding)))
    } else {
        HSTRING::from(label)
    }
}

unsafe fn show_menu(tray: HWND, x: i32, y: i32) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let current = settings::get();
    let check = |on: bool| if on { MF_CHECKED } else { MENU_ITEM_FLAGS(0) };
    let toggle = with_shortcut("Laser on", current.key(Command::Toggle));
    let ink = with_shortcut("Keep strokes", current.key(Command::ToggleInk));
    let clear = with_shortcut("Clear strokes", current.key(Command::Clear));
    let _ = AppendMenuW(menu, MF_STRING | check(crate::laser_on()), MENU_TOGGLE, &toggle);
    let _ = AppendMenuW(menu, MF_STRING | check(current.look.ink), MENU_INK, &ink);
    let grayed = if crate::strokes_on_screen() { MENU_ITEM_FLAGS(0) } else { MF_GRAYED };
    let _ = AppendMenuW(menu, MF_STRING | grayed, MENU_CLEAR, &clear);
    let presets = settings::presets();
    let submenu = CreatePopupMenu().ok();
    if let Some(submenu) = submenu.filter(|_| !presets.is_empty()) {
        let active = settings::active_preset();
        for (index, preset) in presets.iter().enumerate() {
            let label = with_shortcut(&preset.name.replace('&', "&&"), preset.key);
            let _ = AppendMenuW(submenu, MF_STRING | check(active == Some(index)), MENU_PRESET + index, &label);
        }
        let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, submenu.0 as usize, w!("Presets"));
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, w!("Settings\u{2026}"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit"));
    // Without this the menu won't close when the user clicks elsewhere.
    let _ = SetForegroundWindow(tray);
    let chosen = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
        x,
        y,
        None,
        tray,
        None,
    );
    let _ = PostMessageW(Some(tray), WM_NULL, WPARAM(0), LPARAM(0));
    // Destroying the menu also destroys the submenu attached to it.
    let _ = DestroyMenu(menu);
    if let (Some(submenu), true) = (submenu, presets.is_empty()) {
        let _ = DestroyMenu(submenu);
    }
    match chosen.0 as usize {
        MENU_TOGGLE => crate::run_command(Command::Toggle),
        MENU_INK => crate::run_command(Command::ToggleInk),
        MENU_CLEAR => crate::run_command(Command::Clear),
        MENU_SETTINGS => {
            let _ = open_panel();
        }
        MENU_QUIT => crate::quit(),
        id if (MENU_PRESET..MENU_PRESET + presets.len()).contains(&id) => {
            crate::apply_preset(id - MENU_PRESET);
        }
        _ => {}
    }
}

unsafe fn toggle_panel() {
    let flyout = state(|s| s.flyout.get());
    if !flyout.is_invalid() {
        let _ = DestroyWindow(flyout);
        return;
    }
    // Clicking the icon while the panel is open deactivates it first; don't reopen it straight away.
    let just_closed = state(|s| s.closed_at.get())
        .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
    if !just_closed {
        let _ = open_panel();
    }
}

unsafe fn set_attribute<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: T) {
    let _ = DwmSetWindowAttribute(
        hwnd,
        attribute,
        &value as *const T as *const _,
        std::mem::size_of::<T>() as u32,
    );
}

/// Built fresh each time, at the DPI of the monitor it opens on, in the bottom-right corner.
unsafe fn open_panel() -> Result<()> {
    let existing = state(|s| s.flyout.get());
    if !existing.is_invalid() {
        let _ = SetForegroundWindow(existing);
        return Ok(());
    }
    let mut cursor = POINT::default();
    let _ = GetCursorPos(&mut cursor);
    let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = GetMonitorInfoW(monitor, &mut info);
    let (mut dpi, mut _dpi_y) = (96, 96);
    let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);
    let scale = dpi as f32 / 96.0;
    let px = |value: f32| (value * scale).round() as i32;

    let (width, height) = (px(panel::WIDTH), px(panel::HEIGHT));
    let work = info.rcWork;
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        PANEL_CLASS,
        w!("Laser Pointer settings"),
        WS_POPUP,
        work.right - width - px(12.0),
        (work.bottom - height - px(12.0)).max(work.top),
        width,
        height,
        None,
        None,
        Some(state(|s| s.instance.get())),
        None,
    )?;
    let panel = match Panel::new(hwnd, dpi, width as u32, height as u32) {
        Ok(panel) => panel,
        Err(error) => {
            let _ = DestroyWindow(hwnd);
            return Err(error);
        }
    };
    style_window(hwnd, &panel);
    PANEL.with(|slot| *slot.borrow_mut() = Some(panel));
    state(|s| s.flyout.set(hwnd));
    // The panel has the keyboard now; let go of every shortcut so they can be typed into it.
    crate::sync_hotkeys();
    SetTimer(Some(hwnd), TIMER_PREVIEW, 16, None);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    Ok(())
}

unsafe fn style_window(hwnd: HWND, panel: &Panel) {
    set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND);
    set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(panel.theme.dark));
    set_attribute(hwnd, DWMWA_BORDER_COLOR, panel.theme.border_colorref());
}

fn point(lparam: LPARAM, scale: f32) -> (f32, f32) {
    (
        (lparam.0 & 0xffff) as i16 as f32 / scale,
        ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale,
    )
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let scale = with_panel(|panel| panel.scale).unwrap_or(1.0);
        match msg {
            WM_ACTIVATE => {
                // Clicking anywhere else dismisses it, like the system tray flyouts.
                if (wparam.0 & 0xffff) as u32 == WA_INACTIVE && !state(|s| s.modal.get()) {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_PAINT => {
                with_panel(|panel| panel.draw());
                let _ = ValidateRect(Some(hwnd), None);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER if wparam.0 == TIMER_PREVIEW => {
                if with_panel(Panel::tick).unwrap_or(false) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if !state(|s| s.tracking.replace(true)) {
                    let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    });
                }
                let (x, y) = point(lparam, scale);
                act(hwnd, with_panel(|panel| panel.mouse_move(x, y)));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                state(|s| s.tracking.set(false));
                with_panel(Panel::mouse_leave);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
                act(hwnd, with_panel(|panel| panel.scroll(delta)));
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                let (x, y) = point(lparam, scale);
                act(hwnd, with_panel(|panel| panel.mouse_down(x, y)));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                act(hwnd, with_panel(Panel::mouse_up));
                LRESULT(0)
            }
            // Alt+key arrives as a system key; swallow it so the shortcut boxes can record it.
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                act(hwnd, with_panel(|panel| panel.key(wparam.0 as u32)));
                LRESULT(0)
            }
            WM_CHAR => {
                act(hwnd, with_panel(|panel| panel.char(wparam.0 as u32)));
                LRESULT(0)
            }
            WM_SYSCHAR => LRESULT(0),
            WM_SETTINGCHANGE | WM_THEMECHANGED => {
                with_panel(|panel| {
                    panel.reload_theme();
                    style_window(hwnd, panel);
                });
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), TIMER_PREVIEW);
                PANEL.with(|slot| slot.borrow_mut().take());
                state(|s| {
                    s.flyout.set(HWND::default());
                    s.closed_at.set(Some(Instant::now()));
                });
                crate::sync_hotkeys();
                update_tray(false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe fn act(hwnd: HWND, action: Option<Action>) {
    match action {
        Some(Action::Redraw) => {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Some(Action::SettingsChanged) => {
            crate::settings_changed();
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Some(Action::Toggle(on)) => crate::set_laser(on),
        Some(Action::PickColor) => pick_color(hwnd),
        Some(Action::Quit) => crate::quit(),
        Some(Action::Close) => {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        Some(Action::None) | None => {}
    }
}

fn colorref(rgb: u32) -> COLORREF {
    COLORREF((rgb & 0xff) << 16 | (rgb & 0xff00) | (rgb >> 16) & 0xff)
}

unsafe fn pick_color(flyout: HWND) {
    let mut custom = [COLORREF(0xffffff); 16];
    for (slot, color) in custom.iter_mut().zip(panel::SWATCHES) {
        *slot = colorref(color);
    }
    let mut dialog = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: flyout,
        rgbResult: colorref(settings::get().look.color),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // The dialog takes activation; that mustn't dismiss the panel.
    state(|s| s.modal.set(true));
    let picked = ChooseColorW(&mut dialog).as_bool();
    state(|s| s.modal.set(false));
    if picked {
        let mut current = settings::get();
        current.look.color = colorref(dialog.rgbResult.0).0;
        settings::set(current);
        settings::save();
        crate::settings_changed();
    }
    let _ = SetForegroundWindow(flyout);
}
