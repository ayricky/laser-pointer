//! Drawing over fullscreen games. The overlay is an ordinary topmost window: it never injects
//! into or reads another process, so it stays clear of anti-cheat.
//!
//! Three things get in the way. Borderless games are often topmost too and raise themselves,
//! so the overlay re-raises itself whenever another window comes to the front. In mouse-look a
//! game clips the cursor and reads raw input, so drawing would turn the camera; the overlay
//! takes the foreground while the laser is on, which makes the game let go, and hands it back
//! after. And a game in true exclusive fullscreen owns the display outright, which no window can
//! draw over, so the user is told how to switch it to borderless instead.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::{
    ClipCursor, GetClipCursor, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsWindow,
    SetForegroundWindow, SetWindowLongPtrW, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_EX_NOACTIVATE,
};

fn contains(outer: RECT, inner: RECT) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// A game in mouse-look keeps the cursor clipped to part of the screen, and fills its monitor.
/// Slide shows and video players fill the monitor too, but leave the cursor free, so their
/// keyboard shortcuts and clickers keep working while the laser is on.
pub fn captures_mouse(clip: RECT, virtual_screen: RECT, window: RECT, monitor: RECT) -> bool {
    let clipped = !contains(clip, virtual_screen);
    clipped && contains(window, monitor)
}

pub fn install_foreground_hook(callback: unsafe extern "system" fn(HWINEVENTHOOK, u32, HWND, i32, i32, u32, u32)) -> HWINEVENTHOOK {
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    }
}

pub fn remove_hook(hook: HWINEVENTHOOK) {
    if !hook.is_invalid() {
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
    }
}

unsafe fn set_activatable(overlay: HWND, activatable: bool) {
    let style = GetWindowLongPtrW(overlay, GWL_EXSTYLE);
    let flag = WS_EX_NOACTIVATE.0 as isize;
    SetWindowLongPtrW(
        overlay,
        GWL_EXSTYLE,
        if activatable { style & !flag } else { style | flag },
    );
}

/// The window in front, if it's a game holding the mouse.
pub fn mouse_capturing_window(overlay: HWND, virtual_screen: RECT) -> Option<HWND> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() || foreground == overlay {
            return None;
        }
        let mut clip = RECT::default();
        let mut window = RECT::default();
        GetClipCursor(&mut clip).ok()?;
        GetWindowRect(foreground, &mut window).ok()?;
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info).ok().ok()?;
        captures_mouse(clip, virtual_screen, window, info.rcMonitor).then_some(foreground)
    }
}

/// Brings the overlay to the front so the game lets go of the mouse. Returns false if Windows
/// refused, in which case nothing changed.
pub fn take_focus(overlay: HWND) -> bool {
    unsafe {
        set_activatable(overlay, true);
        if !SetForegroundWindow(overlay).as_bool() {
            set_activatable(overlay, false);
            return false;
        }
        let _ = ClipCursor(None);
        true
    }
}

/// Returns the foreground to the game, unless the user has already moved on to something else.
pub fn give_back(overlay: HWND, game: HWND) {
    unsafe {
        set_activatable(overlay, false);
        if GetForegroundWindow() == overlay && IsWindow(Some(game)).as_bool() {
            let _ = SetForegroundWindow(game);
        }
    }
}

/// A Direct3D app is running in exclusive fullscreen, where no window can appear over it.
pub fn exclusive_fullscreen() -> bool {
    unsafe { SHQueryUserNotificationState() }.is_ok_and(|state| state == QUNS_RUNNING_D3D_FULL_SCREEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    const SCREEN: RECT = RECT {
        left: -1920,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const MONITOR: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn a_fullscreen_game_clipping_the_cursor_captures_it() {
        assert!(captures_mouse(rect(959, 539, 961, 541), SCREEN, MONITOR, MONITOR));
        assert!(captures_mouse(MONITOR, SCREEN, rect(-8, -8, 1928, 1088), MONITOR));
    }

    #[test]
    fn an_unclipped_cursor_is_left_alone() {
        // A slide show fills the monitor but leaves the cursor free.
        assert!(!captures_mouse(SCREEN, SCREEN, MONITOR, MONITOR));
    }

    #[test]
    fn a_windowed_app_clipping_the_cursor_is_left_alone() {
        assert!(!captures_mouse(rect(100, 100, 900, 700), SCREEN, rect(100, 100, 900, 700), MONITOR));
    }
}
