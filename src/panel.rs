//! The settings panel: drawn with Direct2D in the Windows 11 style, following the system's
//! light/dark theme and accent colour. Tabs for the pen, presets, shortcuts and options.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, Interface, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DeviceContext5, ID2D1Factory, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1SvgDocument,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VIRTUAL_KEY, VK_BACK, VK_CONTROL,
    VK_ESCAPE, VK_F1, VK_F24, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_TAB,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::settings::{
    self, Autostart, Binding, Command, Look, Preset, Settings, Slot, MAX_NAME, MAX_PRESETS, STYLE_GLOW,
    STYLE_NAMES, STYLE_NEON, STYLE_SOLID,
};
use crate::cursors::{self, STYLES as CURSOR_STYLES};
use crate::stroke::{self, rgba, Point, Scratch, Stroke};
use crate::ui::shortcut_name;

pub const WIDTH: f32 = 420.0;
pub const HEIGHT: f32 = 636.0;
const MARGIN: f32 = 20.0;
const INNER: f32 = WIDTH - 2.0 * MARGIN;
const CONTENT_TOP: f32 = 118.0;
// The shortcuts list scrolls between these; a footer below holds hints and errors.
const LIST_BOTTOM: f32 = HEIGHT - 40.0;
const PREVIEW: D2D_RECT_F = rect(MARGIN, CONTENT_TOP, INNER, 100.0);

pub const SWATCHES: [u32; 6] = [0xff2a2a, 0xff8a00, 0xffd400, 0x2ecc40, 0x1e90ff, 0xd62aff];
const STYLES: [u32; 3] = [STYLE_SOLID, STYLE_GLOW, STYLE_NEON];
const TABS: [(&str, f32); 4] = [("Pen", 56.0), ("Presets", 82.0), ("Shortcuts", 96.0), ("Options", 82.0)];
const PEN: usize = 0;
const PRESETS: usize = 1;
const SHORTCUTS: usize = 2;
const OPTIONS: usize = 3;

const ICON_SAVE: &str = "\u{e74e}";
const ICON_RENAME: &str = "\u{e8ac}";
const ICON_DELETE: &str = "\u{e74d}";
const ICON_ADD: &str = "\u{e710}";
const ICON_CLOSE: &str = "\u{e711}";
const ICON_WARNING: &str = "\u{e7ba}";

struct SliderDef {
    label: &'static str,
    range: (i32, i32),
    get: fn(&Look) -> i32,
    set: fn(&mut Look, i32),
    show: fn(i32) -> String,
}

const FADE: usize = 2;
const SLIDER_TOP: f32 = 414.0;

fn snap_name(value: i32) -> String {
    match value {
        0 => "Off",
        1..=2 => "Strict",
        3..=5 => "Normal",
        _ => "Lenient",
    }
    .into()
}

const SLIDERS: [SliderDef; 5] = [
    SliderDef {
        label: "Thickness",
        range: (2, 24),
        get: |s| s.width as i32,
        set: |s, v| s.width = v as u32,
        show: |v| format!("{v} px"),
    },
    SliderDef {
        label: "Opacity",
        range: (2, 20),
        get: |s| (s.opacity / 5) as i32,
        set: |s, v| s.opacity = v as u32 * 5,
        show: |v| format!("{}%", v * 5),
    },
    SliderDef {
        label: "Fade time",
        range: (2, 50),
        get: |s| (s.fade_ms / 100) as i32,
        set: |s, v| s.fade_ms = v as u32 * 100,
        show: |v| format!("{:.1} s", v as f32 / 10.0),
    },
    SliderDef {
        label: "Smoothing",
        range: (0, 8),
        get: |s| s.smoothing as i32,
        set: |s, v| s.smoothing = v as u32,
        show: |v| v.to_string(),
    },
    // How sloppy a finished stroke may be and still straighten into a line, circle, box or arrow.
    SliderDef {
        label: "Shape snap",
        range: (0, 10),
        get: |s| (s.snap / 10) as i32,
        set: |s, v| s.snap = v as u32 * 10,
        show: snap_name,
    },
];
const TRACK_X: f32 = 128.0;
const TRACK_W: f32 = 208.0;

struct ToggleDef {
    title: &'static str,
    detail: &'static str,
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
}

const KEEP_STROKES: usize = 0;
const AUTOSTART: usize = 4;

const TOGGLES: [ToggleDef; 5] = [
    ToggleDef {
        title: "Keep strokes",
        detail: "Drawings stay up, even with the laser off, until cleared",
        get: |s| s.look.ink,
        set: |s, v| s.look.ink = v,
    },
    ToggleDef {
        title: "Click ripples",
        detail: "Show where right-clicks land",
        get: |s| s.ripples,
        set: |s, v| s.ripples = v,
    },
    ToggleDef {
        title: "On-screen indicator",
        detail: "Confirm toggles, presets and size changes",
        get: |s| s.indicator,
        set: |s, v| s.indicator = v,
    },
    ToggleDef {
        title: "Pause games while drawing",
        detail: "Take the mouse from a game, and hand it back after",
        get: |s| s.game_focus,
        set: |s, v| s.game_focus = v,
    },
    ToggleDef {
        title: "Start with Windows",
        detail: "Ready in the tray after you sign in",
        get: |_| false,
        set: |_, _| {},
    },
];

const POWER: D2D_RECT_F = rect(WIDTH - MARGIN - 44.0, 20.0, 44.0, 22.0);
const RESET: D2D_RECT_F = rect(MARGIN, HEIGHT - 52.0, 160.0, 32.0);
const QUIT: D2D_RECT_F = rect(WIDTH - MARGIN - 100.0, HEIGHT - 52.0, 100.0, 32.0);

#[derive(Clone, Copy, PartialEq)]
enum Widget {
    Power,
    Tab(usize),
    QuickPreset(usize),
    MorePresets,
    Swatch(usize),
    Style(usize),
    Slider(usize),
    PresetRow(usize),
    PresetSave(usize),
    PresetRename(usize),
    PresetDelete(usize),
    NewPreset,
    Binding(Slot),
    Unbind(Slot),
    Cursor(usize),
    Toggle(usize),
    Reset,
    Quit,
}

pub enum Action {
    None,
    Redraw,
    SettingsChanged,
    Toggle(bool),
    /// Something the tray menu and shortcuts do too, done the same way.
    Run(Command),
    PickColor,
    Quit,
    Close,
}

const fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

fn contains(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn hex(rgb: u32) -> D2D1_COLOR_F {
    rgba(rgb, 1.0)
}

fn mix(a: D2D1_COLOR_F, b: D2D1_COLOR_F, k: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: a.r + (b.r - a.r) * k,
        g: a.g + (b.g - a.g) * k,
        b: a.b + (b.b - a.b) * k,
        a: a.a + (b.a - a.a) * k,
    }
}

fn faded(color: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        a: color.a * alpha,
        ..color
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

pub struct Theme {
    pub dark: bool,
    background: D2D1_COLOR_F,
    pub border: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    muted: D2D1_COLOR_F,
    control: D2D1_COLOR_F,
    control_hover: D2D1_COLOR_F,
    control_border: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    on_accent: D2D1_COLOR_F,
    rail: D2D1_COLOR_F,
    danger: D2D1_COLOR_F,
    paper: D2D1_COLOR_F,
    sketch: D2D1_COLOR_F,
}

pub(crate) fn read_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    }
    .is_ok()
    .then_some(data)
}

impl Theme {
    pub fn current() -> Self {
        let light = read_dword(
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
        )
        .is_none_or(|value| value != 0);
        // Stored as 0xAABBGGRR.
        let accent = read_dword(w!(r"Software\Microsoft\Windows\DWM"), w!("AccentColor"))
            .map(|abgr| (abgr & 0xff) << 16 | (abgr & 0xff00) | (abgr >> 16) & 0xff);
        Self::new(light, accent)
    }

    fn new(light: bool, accent: Option<u32>) -> Self {
        if light {
            let accent = accent.map_or(hex(0x005fb8), |rgb| mix(hex(rgb), hex(0x000000), 0.12));
            Self {
                dark: false,
                background: hex(0xf3f3f3),
                border: hex(0xe0e0e0),
                text: hex(0x1a1a1a),
                muted: hex(0x5f5f5f),
                control: hex(0xffffff),
                control_hover: hex(0xf6f6f6),
                control_border: hex(0xd4d4d4),
                accent,
                on_accent: hex(0xffffff),
                rail: hex(0xc6c6c6),
                danger: hex(0xc42b1c),
                paper: hex(0xffffff),
                sketch: hex(0xe3e7ef),
            }
        } else {
            let accent = accent.map_or(hex(0x60cdff), |rgb| mix(hex(rgb), hex(0xffffff), 0.45));
            Self {
                dark: true,
                background: hex(0x202020),
                border: hex(0x3a3a3a),
                text: hex(0xffffff),
                muted: hex(0xa8a8a8),
                control: hex(0x373737),
                control_hover: hex(0x3f3f3f),
                control_border: hex(0x484848),
                accent,
                on_accent: hex(0x000000),
                rail: hex(0x6a6a6a),
                danger: hex(0xff99a4),
                paper: hex(0x1b1d22),
                sketch: hex(0x3a3f4a),
            }
        }
    }

    pub fn border_colorref(&self) -> COLORREF {
        let c = self.border;
        let byte = |v: f32| (v * 255.0).round() as u32;
        COLORREF(byte(c.b) << 16 | byte(c.g) << 8 | byte(c.r))
    }
}

struct Fonts {
    title: IDWriteTextFormat,
    body: IDWriteTextFormat,
    body_centre: IDWriteTextFormat,
    body_right: IDWriteTextFormat,
    small: IDWriteTextFormat,
    small_line: IDWriteTextFormat,
    section: IDWriteTextFormat,
    icon: IDWriteTextFormat,
}

struct Preview {
    points: Vec<Point>,
    t: f32,
    last: Instant,
    scratch: Scratch,
}

impl Preview {
    fn position(t: f32) -> (f32, f32) {
        let cx = (PREVIEW.left + PREVIEW.right) / 2.0;
        let cy = (PREVIEW.top + PREVIEW.bottom) / 2.0 + 6.0;
        (cx + 130.0 * t.sin(), cy + 26.0 * (2.0 * t).sin())
    }
}

struct Rename {
    index: usize,
    text: String,
    /// Typing replaces the suggested name rather than appending to it.
    fresh: bool,
}

/// A line in the shortcuts list.
enum Line {
    Note,
    Section(&'static str),
    Item(Slot, String, &'static str),
    /// A gesture that can't be rebound, listed so it can be found: label, keys, when.
    Fixed(&'static str, &'static str, &'static str),
}

const ROW: f32 = 44.0;

fn line_height(line: &Line) -> f32 {
    match line {
        Line::Note => 38.0,
        Line::Section(_) => 30.0,
        Line::Item(..) | Line::Fixed(..) => ROW,
    }
}

/// The cursor picker on the Options tab, above the switches.
const CURSOR_SWATCH: f32 = 40.0;
const TOGGLES_TOP: f32 = CONTENT_TOP + 80.0;

pub struct Panel {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    target: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
    fonts: Fonts,
    pub theme: Theme,
    pub scale: f32,
    tab: usize,
    hover: Option<Widget>,
    dragging: Option<usize>,
    capturing: Option<Slot>,
    renaming: Option<Rename>,
    status: Option<(String, Instant, bool)>,
    scroll: f32,
    started: Instant,
    preview: Preview,
    /// The window drawn into, with its DPI and pixel size, to rebuild the target after a GPU
    /// reset; None for snapshots.
    window: Option<(HWND, u32, u32, u32)>,
    /// For drawing cursor styles; None before Windows 10 1703, which lacks SVG support.
    svg: Option<ID2D1DeviceContext5>,
    cursor_documents: std::cell::RefCell<Vec<((usize, u32), ID2D1SvgDocument)>>,
}

unsafe fn text_format(
    dwrite: &IDWriteFactory,
    family: PCWSTR,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
    align: DWRITE_TEXT_ALIGNMENT,
    paragraph: DWRITE_PARAGRAPH_ALIGNMENT,
    wrap: bool,
) -> Result<IDWriteTextFormat> {
    let format = dwrite.CreateTextFormat(
        family,
        None,
        weight,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        size,
        w!("en-us"),
    )?;
    format.SetTextAlignment(align)?;
    format.SetParagraphAlignment(paragraph)?;
    format.SetWordWrapping(if wrap {
        DWRITE_WORD_WRAPPING_WRAP
    } else {
        DWRITE_WORD_WRAPPING_NO_WRAP
    })?;
    if !wrap {
        // Long names end in an ellipsis instead of running into the buttons.
        let sign = dwrite.CreateEllipsisTrimmingSign(&format)?;
        format.SetTrimming(
            &DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                ..Default::default()
            },
            &sign,
        )?;
    }
    Ok(format)
}

/// Segoe Fluent Icons ships with Windows 11; Windows 10 has the same glyphs in MDL2.
unsafe fn icon_family(dwrite: &IDWriteFactory) -> PCWSTR {
    let mut collection: Option<IDWriteFontCollection> = None;
    let mut index = 0;
    let mut exists = BOOL::default();
    let found = dwrite.GetSystemFontCollection(&mut collection, false).is_ok()
        && collection.is_some_and(|fonts| {
            fonts
                .FindFamilyName(w!("Segoe Fluent Icons"), &mut index, &mut exists)
                .is_ok()
                && exists.as_bool()
        });
    if found {
        w!("Segoe Fluent Icons")
    } else {
        w!("Segoe MDL2 Assets")
    }
}

fn modifiers_held() -> u32 {
    let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    let mut mods = 0;
    if held(VK_CONTROL) {
        mods |= MOD_CONTROL.0;
    }
    if held(VK_MENU) {
        mods |= MOD_ALT.0;
    }
    if held(VK_SHIFT) {
        mods |= MOD_SHIFT.0;
    }
    if held(VK_LWIN) || held(VK_RWIN) {
        mods |= MOD_WIN.0;
    }
    mods
}

/// Where "Start with Windows" stands, as last reported; the Store build hears back on another
/// thread.
static AUTOSTART_STATE: Mutex<Autostart> = Mutex::new(Autostart::Unknown);
/// Set while Windows is answering, so requests run one at a time and in the order made.
static AUTOSTART_BUSY: AtomicBool = AtomicBool::new(false);

fn autostart_state() -> Autostart {
    *AUTOSTART_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Panel {
    /// Reads, or changes and then reads, "Start with Windows", and repaints when the answer comes.
    /// Ignored while an earlier request is still out; its answer is on the way.
    fn ask_autostart(&self, change: Option<bool>) {
        if AUTOSTART_BUSY.swap(true, Ordering::AcqRel) {
            return;
        }
        let window = self.window.map(|(hwnd, ..)| hwnd.0 as isize);
        settings::autostart(change, move |state| {
            *AUTOSTART_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
            AUTOSTART_BUSY.store(false, Ordering::Release);
            if let Some(window) = window {
                // Safe from any thread; a window closed in the meantime just ignores it.
                unsafe {
                    let _ = InvalidateRect(Some(HWND(window as _)), None, false);
                }
            }
        });
    }

    pub unsafe fn new(hwnd: HWND, dpi: u32, width: u32, height: u32) -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let target = Self::window_target(&factory, hwnd, dpi, width, height)?;
        let mut panel = Self::with_target(factory, target, dpi, Theme::current())?;
        panel.window = Some((hwnd, dpi, width, height));
        panel.ask_autostart(None);
        Ok(panel)
    }

    unsafe fn window_target(
        factory: &ID2D1Factory,
        hwnd: HWND,
        dpi: u32,
        width: u32,
        height: u32,
    ) -> Result<ID2D1RenderTarget> {
        factory.CreateHwndRenderTarget(
            &D2D1_RENDER_TARGET_PROPERTIES {
                dpiX: dpi as f32,
                dpiY: dpi as f32,
                ..Default::default()
            },
            &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U { width, height },
                // Waiting for the display here would hold up the overlay, which shares the thread.
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            },
        )?
        .cast()
    }

    /// The GPU was reset: draw into a fresh target, keeping everything else about the panel.
    unsafe fn rebuild_target(&mut self) -> Result<()> {
        let Some((hwnd, dpi, width, height)) = self.window else {
            return Ok(());
        };
        self.target = Self::window_target(&self.factory, hwnd, dpi, width, height)?;
        self.brush = self.target.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
        self.svg = self.target.cast().ok();
        self.cursor_documents.borrow_mut().clear();
        Ok(())
    }

    /// Draws onto any render target: the panel's window, or a bitmap in the snapshot tests.
    unsafe fn with_target(
        factory: ID2D1Factory,
        target: ID2D1RenderTarget,
        dpi: u32,
        theme: Theme,
    ) -> Result<Self> {
        let brush = target.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let display = w!("Segoe UI Variable Display");
        let body = w!("Segoe UI Variable Text");
        let (leading, centre, trailing) = (
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
        );
        let (middle, near) = (DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
        let (regular, semibold) = (DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD);
        let fonts = Fonts {
            title: text_format(&dwrite, display, semibold, 20.0, leading, middle, false)?,
            body: text_format(&dwrite, body, regular, 14.0, leading, middle, false)?,
            body_centre: text_format(&dwrite, body, regular, 14.0, centre, middle, false)?,
            body_right: text_format(&dwrite, body, regular, 14.0, trailing, middle, false)?,
            small: text_format(&dwrite, body, regular, 12.0, leading, near, true)?,
            small_line: text_format(&dwrite, body, regular, 12.0, leading, middle, false)?,
            section: text_format(&dwrite, body, semibold, 14.0, leading, middle, false)?,
            icon: text_format(&dwrite, icon_family(&dwrite), regular, 14.0, centre, middle, false)?,
        };
        let svg: Option<ID2D1DeviceContext5> = target.cast().ok();
        Ok(Self {
            factory,
            dwrite,
            target,
            brush,
            fonts,
            theme,
            scale: dpi as f32 / 96.0,
            tab: PEN,
            hover: None,
            dragging: None,
            capturing: None,
            renaming: None,
            status: None,
            scroll: 0.0,
            started: Instant::now(),
            preview: Preview {
                points: Vec::new(),
                t: 0.0,
                last: Instant::now(),
                scratch: Scratch::default(),
            },
            window: None,
            svg,
            cursor_documents: Default::default(),
        })
    }

    pub fn reload_theme(&mut self) {
        self.theme = Theme::current();
    }

    /// Recording a shortcut or typing a preset's name, so the app's own shortcuts must let go.
    pub fn wants_keys(&self) -> bool {
        self.capturing.is_some() || self.renaming.is_some()
    }

    /// Advances the preview's pretend hand along a figure of eight. Returns whether anything on
    /// screen is moving, so an idle tab isn't repainted sixty times a second.
    pub fn tick(&mut self) -> bool {
        self.scroll = self.scroll.min(Self::max_scroll());
        let status_showing = self
            .status
            .as_ref()
            .is_some_and(|(_, at, _)| at.elapsed() < Duration::from_millis(5100));
        if self.tab != PEN {
            return self.renaming.is_some() || status_showing;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.preview.last).as_secs_f32().min(0.1);
        self.preview.last = now;
        let before = self.preview.t;
        let after = before + dt * 1.8;
        self.preview.t = after;
        if settings::get().look.ink && (after / TAU).floor() > (before / TAU).floor() {
            self.preview.points.clear();
        }
        let (x0, y0) = Preview::position(before);
        let (x1, y1) = Preview::position(after);
        let steps = (((x1 - x0).hypot(y1 - y0) / 2.5).ceil() as usize).max(1);
        for step in 1..=steps {
            let t = before + (after - before) * step as f32 / steps as f32;
            let (x, y) = Preview::position(t);
            self.preview.points.push(Point { x, y, at: now });
        }
        true
    }

    fn say(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some((text.into(), Instant::now(), error));
    }

    // ---- Layout ---------------------------------------------------------------------------------

    fn tab_rects() -> [D2D_RECT_F; 4] {
        let mut x = 12.0;
        TABS.map(|(_, width)| {
            let area = rect(x, 66.0, width, 38.0);
            x += width;
            area
        })
    }

    fn quick_presets() -> (Vec<usize>, bool) {
        let count = settings::presets().len();
        if count <= 4 {
            ((0..count).collect(), false)
        } else {
            ((0..3).collect(), true)
        }
    }

    fn preset_row(index: usize) -> D2D_RECT_F {
        rect(MARGIN, 158.0 + index as f32 * 48.0, INNER, 44.0)
    }

    fn lines() -> Vec<Line> {
        let mut lines = vec![Line::Note, Line::Section("Laser")];
        for command in [
            Command::Toggle,
            Command::Off,
            Command::Undo,
            Command::Redo,
            Command::Clear,
            Command::ToggleInk,
            Command::Quit,
        ] {
            lines.push(Line::Item(
                Slot::Command(command),
                command.label().into(),
                command.scope(),
            ));
        }
        lines.push(Line::Section("Presets"));
        for command in [Command::NextPreset, Command::PreviousPreset] {
            lines.push(Line::Item(
                Slot::Command(command),
                command.label().into(),
                command.scope(),
            ));
        }
        for (index, preset) in settings::presets().iter().enumerate() {
            lines.push(Line::Item(Slot::Preset(index), preset.name.clone(), "switches to this preset"));
        }
        lines.push(Line::Section("While drawing"));
        lines.push(Line::Fixed("Straight horizontal line", "Alt", "hold while drawing"));
        lines.push(Line::Fixed("Straight vertical line", "Shift+Alt", "hold while drawing"));
        lines.push(Line::Fixed("Skip shape snapping", "Ctrl", "hold as you let go"));
        lines
    }

    fn list_height() -> f32 {
        Self::lines().iter().map(line_height).sum::<f32>() + 8.0
    }

    fn max_scroll() -> f32 {
        (Self::list_height() - (LIST_BOTTOM - CONTENT_TOP)).max(0.0)
    }

    fn binding_box(y: f32) -> (D2D_RECT_F, D2D_RECT_F) {
        (
            rect(WIDTH - MARGIN - 36.0 - 150.0, y + 7.0, 150.0, 30.0),
            rect(WIDTH - MARGIN - 30.0, y + 7.0, 30.0, 30.0),
        )
    }

    fn layout(&self) -> Vec<(Widget, D2D_RECT_F)> {
        let mut widgets = vec![(Widget::Power, POWER)];
        for (index, area) in Self::tab_rects().into_iter().enumerate() {
            widgets.push((Widget::Tab(index), area));
        }
        match self.tab {
            PEN => {
                let (quick, more) = Self::quick_presets();
                let count = quick.len() + usize::from(more);
                let width = (INNER - 8.0 * (count.max(1) - 1) as f32) / count.max(1) as f32;
                for (slot, index) in quick.iter().enumerate() {
                    let x = MARGIN + slot as f32 * (width + 8.0);
                    widgets.push((Widget::QuickPreset(*index), rect(x, 252.0, width, 30.0)));
                }
                if more {
                    let x = MARGIN + quick.len() as f32 * (width + 8.0);
                    widgets.push((Widget::MorePresets, rect(x, 252.0, width, 30.0)));
                }
                let gap = (INNER - 7.0 * 28.0) / 6.0;
                for index in 0..=SWATCHES.len() {
                    let x = MARGIN + index as f32 * (28.0 + gap);
                    widgets.push((Widget::Swatch(index), rect(x, 316.0, 28.0, 28.0)));
                }
                let third = INNER / 3.0;
                for index in 0..STYLES.len() {
                    let x = MARGIN + index as f32 * third;
                    widgets.push((Widget::Style(index), rect(x, 376.0, third, 30.0)));
                }
                for index in 0..SLIDERS.len() {
                    let y = SLIDER_TOP + index as f32 * 34.0;
                    widgets.push((Widget::Slider(index), rect(TRACK_X - 10.0, y, TRACK_W + 20.0, 34.0)));
                }
            }
            PRESETS => {
                let count = settings::presets().len();
                for index in 0..count {
                    let row = Self::preset_row(index);
                    let icon = |offset: f32| rect(row.right - offset, row.top + 8.0, 28.0, 28.0);
                    widgets.push((Widget::PresetSave(index), icon(100.0)));
                    widgets.push((Widget::PresetRename(index), icon(68.0)));
                    widgets.push((Widget::PresetDelete(index), icon(36.0)));
                    widgets.push((Widget::PresetRow(index), row));
                }
                if count < MAX_PRESETS {
                    let top = Self::preset_row(count).top + 4.0;
                    widgets.push((Widget::NewPreset, rect(MARGIN, top, INNER, 34.0)));
                }
            }
            SHORTCUTS => {
                let mut y = CONTENT_TOP - self.scroll;
                for line in Self::lines() {
                    if let Line::Item(slot, ..) = line {
                        let (field, clear) = Self::binding_box(y);
                        if field.top >= CONTENT_TOP - 4.0 && field.bottom <= LIST_BOTTOM + 4.0 {
                            widgets.push((Widget::Binding(slot), field));
                            if settings::binding(slot).is_set() {
                                widgets.push((Widget::Unbind(slot), clear));
                            }
                        }
                    }
                    y += line_height(&line);
                }
            }
            OPTIONS => {
                let count = CURSOR_STYLES.len() as f32;
                let gap = (INNER - count * CURSOR_SWATCH) / (count - 1.0);
                for index in 0..CURSOR_STYLES.len() {
                    let x = MARGIN + index as f32 * (CURSOR_SWATCH + gap);
                    widgets.push((Widget::Cursor(index), rect(x, CONTENT_TOP + 26.0, CURSOR_SWATCH, CURSOR_SWATCH)));
                }
                for index in 0..TOGGLES.len() {
                    widgets.push((Widget::Toggle(index), rect(MARGIN - 8.0, TOGGLES_TOP + index as f32 * 52.0, INNER + 16.0, 48.0)));
                }
                widgets.push((Widget::Reset, RESET));
                widgets.push((Widget::Quit, QUIT));
            }
            _ => {}
        }
        widgets
    }

    fn hit(&self, x: f32, y: f32) -> Option<Widget> {
        self.layout()
            .into_iter()
            .find(|(_, area)| contains(area, x, y))
            .map(|(widget, _)| widget)
    }

    // ---- Input ----------------------------------------------------------------------------------

    pub fn mouse_move(&mut self, x: f32, y: f32) -> Action {
        if let Some(index) = self.dragging {
            return self.slide(index, x);
        }
        let hover = self.hit(x, y);
        if hover == self.hover {
            return Action::None;
        }
        self.hover = hover;
        Action::Redraw
    }

    pub fn mouse_leave(&mut self) {
        self.hover = None;
    }

    pub fn scroll(&mut self, delta: i16) -> Action {
        if self.tab != SHORTCUTS {
            return Action::None;
        }
        self.scroll = (self.scroll - f32::from(delta) / 120.0 * ROW).clamp(0.0, Self::max_scroll());
        Action::Redraw
    }

    pub fn mouse_down(&mut self, x: f32, y: f32) -> Action {
        let widget = self.hit(x, y);
        let renaming = self.renaming.as_ref().map(|rename| rename.index);
        if renaming.is_some() && widget != renaming.map(Widget::PresetRow) {
            self.finish_rename(true);
        }
        if self.capturing.is_some() && !matches!(widget, Some(Widget::Binding(_))) {
            self.capturing = None;
        }
        let Some(widget) = widget else {
            return Action::Redraw;
        };
        let mut current = settings::get();
        match widget {
            Widget::Power => return Action::Toggle(!crate::laser_on()),
            Widget::Tab(index) => {
                self.tab = index;
                self.status = None;
                self.scroll = 0.0;
                return Action::Redraw;
            }
            Widget::QuickPreset(index) | Widget::PresetRow(index) => {
                if renaming == Some(index) {
                    return Action::None;
                }
                settings::apply_preset(index);
                settings::save();
                return Action::SettingsChanged;
            }
            Widget::MorePresets => {
                self.tab = PRESETS;
                return Action::Redraw;
            }
            Widget::Swatch(index) if index < SWATCHES.len() => current.look.color = SWATCHES[index],
            Widget::Swatch(_) => return Action::PickColor,
            Widget::Style(index) => current.look.style = STYLES[index],
            Widget::Cursor(index) => current.cursor = index,
            Widget::Slider(index) => {
                if !Self::slider_enabled(index, &current) {
                    return Action::None;
                }
                self.dragging = Some(index);
                return self.slide(index, x);
            }
            Widget::PresetSave(index) => {
                let mut list = settings::presets();
                list[index].look = current.look;
                let name = list[index].name.clone();
                settings::set_presets(list);
                settings::save();
                self.say(format!("Updated \u{201c}{name}\u{201d} to the current pen"), false);
                return Action::SettingsChanged;
            }
            Widget::PresetRename(index) => {
                let text = settings::presets()[index].name.clone();
                self.renaming = Some(Rename {
                    index,
                    text,
                    fresh: true,
                });
                return Action::Redraw;
            }
            Widget::PresetDelete(index) => {
                let mut list = settings::presets();
                let removed = list.remove(index);
                settings::set_presets(list);
                settings::save();
                self.say(format!("Deleted \u{201c}{}\u{201d}", removed.name), false);
                return Action::SettingsChanged;
            }
            Widget::NewPreset => {
                let mut list = settings::presets();
                let name = (1..)
                    .map(|n| format!("Preset {n}"))
                    .find(|name| !list.iter().any(|preset| &preset.name == name))
                    .unwrap_or_default();
                list.push(Preset {
                    name: name.clone(),
                    look: current.look,
                    key: Binding::NONE,
                });
                settings::set_presets(list);
                settings::save();
                self.renaming = Some(Rename {
                    index: settings::presets().len() - 1,
                    text: name,
                    fresh: true,
                });
                return Action::SettingsChanged;
            }
            Widget::Binding(slot) => {
                self.capturing = if self.capturing == Some(slot) { None } else { Some(slot) };
                self.status = None;
                return Action::Redraw;
            }
            Widget::Unbind(slot) => {
                settings::set_binding(slot, Binding::NONE);
                self.capturing = None;
                return Action::SettingsChanged;
            }
            // Turning it off also lets go of what is kept on screen, as the shortcut does.
            Widget::Toggle(KEEP_STROKES) => return Action::Run(Command::ToggleInk),
            Widget::Toggle(AUTOSTART) => {
                match autostart_state() {
                    Autostart::Policy(_) | Autostart::Unknown => {}
                    state => self.ask_autostart(Some(state != Autostart::On)),
                }
                return Action::Redraw;
            }
            Widget::Toggle(index) => {
                let toggle = &TOGGLES[index];
                let on = (toggle.get)(&current);
                (toggle.set)(&mut current, !on);
            }
            Widget::Reset => {
                let keys = current.keys;
                current = Settings::default();
                current.keys = keys;
                self.say("Pen and options reset; shortcuts and presets kept", false);
            }
            Widget::Quit => return Action::Quit,
        }
        settings::set(current);
        settings::save();
        Action::SettingsChanged
    }

    pub fn mouse_up(&mut self) -> Action {
        if self.dragging.take().is_some() {
            settings::save();
            return Action::SettingsChanged;
        }
        Action::None
    }

    fn slider_enabled(index: usize, settings: &Settings) -> bool {
        match index {
            FADE => !settings.look.ink,
            _ => true,
        }
    }

    fn slide(&mut self, index: usize, x: f32) -> Action {
        let slider = &SLIDERS[index];
        let (min, max) = slider.range;
        let k = ((x - TRACK_X) / TRACK_W).clamp(0.0, 1.0);
        let value = min + (k * (max - min) as f32).round() as i32;
        let mut current = settings::get();
        (slider.set)(&mut current.look, value);
        settings::set(current);
        Action::Redraw
    }

    fn finish_rename(&mut self, keep: bool) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        let name = settings::clean_name(&rename.text);
        if !keep || name.is_empty() {
            return;
        }
        let mut list = settings::presets();
        if let Some(preset) = list.get_mut(rename.index) {
            preset.name = name;
        }
        settings::set_presets(list);
        settings::save();
    }

    pub fn char(&mut self, unit: u32) -> Action {
        let Some(rename) = self.renaming.as_mut() else {
            return Action::None;
        };
        let Some(ch) = char::from_u32(unit) else {
            return Action::None;
        };
        if ch.is_control() || ch == '|' {
            return Action::None;
        }
        if rename.fresh {
            rename.text.clear();
            rename.fresh = false;
        }
        if rename.text.chars().count() < MAX_NAME {
            rename.text.push(ch);
        }
        Action::Redraw
    }

    pub fn key(&mut self, vk: u32) -> Action {
        let is = |key: VIRTUAL_KEY| vk == u32::from(key.0);
        if let Some(rename) = self.renaming.as_mut() {
            if is(VK_RETURN) {
                self.finish_rename(true);
                return Action::SettingsChanged;
            }
            if is(VK_ESCAPE) {
                self.finish_rename(false);
            } else if is(VK_BACK) {
                if rename.fresh {
                    rename.text.clear();
                    rename.fresh = false;
                } else {
                    rename.text.pop();
                }
            }
            return Action::Redraw;
        }
        if let Some(slot) = self.capturing {
            return self.capture(slot, vk);
        }
        if is(VK_ESCAPE) {
            return Action::Close;
        }
        // Ctrl+Tab moves between tabs, like most tabbed windows.
        if is(VK_TAB) && modifiers_held() & MOD_CONTROL.0 != 0 {
            let back = modifiers_held() & MOD_SHIFT.0 != 0;
            self.tab = (self.tab + if back { TABS.len() - 1 } else { 1 }) % TABS.len();
            self.scroll = 0.0;
            return Action::Redraw;
        }
        Action::None
    }

    fn capture(&mut self, slot: Slot, vk: u32) -> Action {
        let modifiers = [
            VK_SHIFT, VK_CONTROL, VK_MENU, VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU,
            VK_RMENU, VK_LWIN, VK_RWIN,
        ];
        if modifiers.iter().any(|key| u32::from(key.0) == vk) {
            return Action::None;
        }
        let binding = Binding {
            mods: modifiers_held(),
            vk,
        };
        let name = shortcut_name(binding);
        if let Some(owner) = settings::binding_owner(binding, slot) {
            self.say(format!("{name} is already used by {owner}"), true);
            return Action::Redraw;
        }
        if !crate::shortcut_free(binding) {
            self.say(format!("{name} is taken by another app"), true);
            return Action::Redraw;
        }
        settings::set_binding(slot, binding);
        self.capturing = None;
        let always = matches!(slot, Slot::Preset(_))
            || matches!(slot, Slot::Command(command) if command.scope() == "always");
        let function_key = (u32::from(VK_F1.0)..=u32::from(VK_F24.0)).contains(&vk);
        if always && binding.mods == 0 && !function_key {
            self.say(format!("Saved. {name} on its own won't type in other apps any more"), false);
        } else {
            self.say(format!("Saved {name}"), false);
        }
        Action::SettingsChanged
    }

    // ---- Drawing --------------------------------------------------------------------------------

    /// Draws the panel. After a GPU reset it rebuilds its target and tries once more.
    pub unsafe fn draw(&mut self) {
        // D2DERR_RECREATE_TARGET: the device behind the window's target went away.
        let lost = |error: &windows::core::Error| error.code().0 as u32 == 0x8899_000C;
        if self.draw_once().is_err_and(|error| lost(&error)) && self.rebuild_target().is_ok() {
            let _ = self.draw_once();
        }
    }

    unsafe fn draw_once(&mut self) -> Result<()> {
        let settings = settings::get();
        self.target.BeginDraw();
        self.target.Clear(Some(&self.theme.background));
        self.draw_header(&settings);
        match self.tab {
            PEN => self.draw_pen(&settings),
            PRESETS => self.draw_presets(&settings),
            SHORTCUTS => self.draw_shortcuts(),
            OPTIONS => self.draw_options(&settings),
            _ => {}
        }
        self.target.EndDraw(None, None)
    }

    unsafe fn draw_header(&self, settings: &Settings) {
        let theme = &self.theme;
        let on = crate::laser_on();
        let dot = if on { hex(settings.look.color) } else { theme.rail };
        if on {
            self.circle(28.0, 29.0, 11.0, faded(dot, 0.2));
        }
        self.circle(28.0, 29.0, 6.0, dot);
        self.text("Laser Pointer", &self.fonts.title, rect(44.0, 14.0, 240.0, 30.0), theme.text);
        let mut hints = Vec::new();
        if settings.key(Command::Toggle).is_set() {
            hints.push(format!("{} to toggle", shortcut_name(settings.key(Command::Toggle))));
        }
        if settings.key(Command::Off).is_set() {
            hints.push(format!("{} to stop", shortcut_name(settings.key(Command::Off))));
        }
        let hint = if hints.is_empty() {
            "No shortcuts set \u{2014} see the Shortcuts tab".to_string()
        } else {
            hints.join("  \u{b7}  ")
        };
        self.text(&hint, &self.fonts.small_line, rect(MARGIN, 42.0, 300.0, 18.0), theme.muted);
        self.text(
            if on { "On" } else { "Off" },
            &self.fonts.body_right,
            rect(POWER.left - 58.0, POWER.top, 50.0, 22.0),
            theme.text,
        );
        self.switch(POWER, on, self.hover == Some(Widget::Power));
        for (index, area) in Self::tab_rects().into_iter().enumerate() {
            let selected = index == self.tab;
            let hovered = self.hover == Some(Widget::Tab(index));
            let color = if selected || hovered { theme.text } else { theme.muted };
            let format = if selected { &self.fonts.section } else { &self.fonts.body };
            let label = rect(area.left, area.top, area.right - area.left, 30.0);
            self.text(TABS[index].0, format, label, color);
            if selected {
                // Under the label, which sits at the left of its tab, not the tab's middle.
                let cx = area.left + self.measure(TABS[index].0, format) / 2.0;
                self.fill_round(rect(cx - 8.0, area.bottom - 7.0, 16.0, 3.0), 1.5, theme.accent);
            }
        }
        self.fill(rect(0.0, 106.0, WIDTH, 1.0), theme.border);
    }

    unsafe fn draw_pen(&mut self, settings: &Settings) {
        self.draw_preview(&settings.look, settings.cursor);
        let theme = &self.theme;
        self.section("Presets", 230.0);
        let active = settings::active_preset();
        let presets = settings::presets();
        self.section("Colour", 294.0);
        self.section("Style", 354.0);
        for (widget, area) in self.layout() {
            let hovered = self.hover == Some(widget);
            match widget {
                Widget::QuickPreset(index) => {
                    self.chip(area, &presets[index].name, active == Some(index), hovered, 6.0);
                }
                Widget::MorePresets => self.chip(area, "More\u{2026}", false, hovered, 6.0),
                Widget::Swatch(index) => self.swatch(area, index, settings.look.color, hovered),
                Widget::Style(index) => {
                    let selected = settings.look.style == STYLES[index];
                    self.segment(area, STYLE_NAMES[index], selected, hovered, index);
                }
                Widget::Slider(index) => self.slider(index, settings, hovered),
                _ => {}
            }
        }
        if presets.is_empty() {
            self.text("No presets yet", &self.fonts.small_line, rect(MARGIN, 252.0, INNER, 30.0), theme.muted);
        }
    }

    unsafe fn draw_preview(&mut self, look: &Look, cursor: usize) {
        let card = D2D1_ROUNDED_RECT {
            rect: PREVIEW,
            radiusX: 8.0,
            radiusY: 8.0,
        };
        self.brush.SetColor(&self.theme.paper);
        self.target.FillRoundedRectangle(&card, &self.brush);
        // A pretend slide underneath.
        let sketch = self.theme.sketch;
        self.fill_round(rect(PREVIEW.left + 16.0, PREVIEW.top + 14.0, 140.0, 10.0), 3.0, sketch);
        for row in 0..3 {
            let y = PREVIEW.top + 38.0 + row as f32 * 15.0;
            self.fill_round(rect(PREVIEW.left + 16.0, y, 170.0 - row as f32 * 30.0, 6.0), 3.0, sketch);
        }
        for bar in 0..4 {
            let h = [24.0, 36.0, 30.0, 48.0][bar];
            let x = PREVIEW.right - 100.0 + bar as f32 * 20.0;
            self.fill_round(rect(x, PREVIEW.bottom - 14.0 - h, 12.0, h), 2.0, sketch);
        }
        self.target
            .PushAxisAlignedClip(&PREVIEW, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        let mut preview = Stroke {
            points: std::mem::take(&mut self.preview.points),
            look: *look,
            scale: 1.0,
            whole: false,
            breaks: Vec::new(),
        };
        stroke::draw(
            &self.factory,
            &self.target,
            &self.brush,
            &mut preview,
            Instant::now(),
            1.0,
            &mut self.preview.scratch,
        );
        self.preview.points = preview.points;
        let (hx, hy) = Preview::position(self.preview.t);
        let (sx, sy) = CURSOR_STYLES[cursor].hotspot;
        let zoom = 0.75;
        self.draw_cursor(cursor, look.color, hx - sx * zoom, hy - sy * zoom, zoom);
        self.target.PopAxisAlignedClip();
        self.brush.SetColor(&self.theme.border);
        self.target.DrawRoundedRectangle(&card, &self.brush, 1.0, None);
    }

    unsafe fn draw_presets(&self, settings: &Settings) {
        let theme = &self.theme;
        self.text(
            "Click a preset to switch to it. Give each one its own shortcut on the Shortcuts tab.",
            &self.fonts.small,
            rect(MARGIN, CONTENT_TOP, INNER, 34.0),
            theme.muted,
        );
        let presets = settings::presets();
        let active = presets.iter().position(|preset| preset.look == settings.look);
        for (widget, area) in self.layout() {
            let hovered = self.hover == Some(widget);
            match widget {
                Widget::PresetRow(index) => {
                    self.preset_card(area, &presets[index], index, active == Some(index), hovered);
                }
                Widget::NewPreset => {
                    self.button(area, "", hovered, theme.text);
                    self.icon(ICON_ADD, rect(area.left + 10.0, area.top, 24.0, 34.0), theme.text);
                    self.text("New preset from the current pen", &self.fonts.body, rect(area.left + 38.0, area.top, 300.0, 34.0), theme.text);
                }
                _ => {}
            }
        }
        // Icons last, so they sit on top of their rows.
        for (widget, area) in self.layout() {
            let hovered = self.hover == Some(widget);
            let (glyph, color, tip) = match widget {
                Widget::PresetSave(_) => (ICON_SAVE, theme.text, "Overwrite with the current pen"),
                Widget::PresetRename(_) => (ICON_RENAME, theme.text, "Rename"),
                Widget::PresetDelete(_) => (ICON_DELETE, theme.danger, "Delete"),
                _ => continue,
            };
            if hovered {
                self.fill_round(area, 4.0, theme.control_hover);
                self.text(tip, &self.fonts.small_line, rect(MARGIN, LIST_BOTTOM + 8.0, INNER, 20.0), theme.muted);
            }
            self.icon(glyph, area, color);
        }
        if presets.len() >= MAX_PRESETS {
            self.text("That's the most presets there's room for.", &self.fonts.small_line, rect(MARGIN, LIST_BOTTOM + 8.0, INNER, 20.0), theme.muted);
        }
        self.draw_status();
    }

    unsafe fn preset_card(&self, area: D2D_RECT_F, preset: &Preset, index: usize, active: bool, hovered: bool) {
        let theme = &self.theme;
        let fill = if hovered { theme.control_hover } else { theme.control };
        self.fill_round(area, 6.0, fill);
        if active {
            self.outline_round(area, 6.0, theme.accent, 1.5);
            self.fill_round(rect(area.left + 1.5, area.top + 12.0, 3.0, 20.0), 1.5, theme.accent);
        } else {
            self.outline_round(area, 6.0, theme.control_border, 1.0);
        }
        let color = hex(preset.look.color);
        let cy = (area.top + area.bottom) / 2.0;
        if preset.look.style != STYLE_SOLID {
            self.circle(area.left + 22.0, cy, 12.0, faded(color, 0.22));
        }
        self.circle(area.left + 22.0, cy, 8.0, color);
        let name_area = rect(area.left + 42.0, area.top + 3.0, 196.0, 22.0);
        match &self.renaming {
            Some(rename) if rename.index == index => {
                let field = rect(name_area.left - 4.0, name_area.top, name_area.right - name_area.left + 4.0, 22.0);
                self.fill_round(field, 3.0, theme.background);
                self.outline_round(field, 3.0, theme.accent, 1.5);
                let shown = rename.text.as_str();
                let text_area = rect(field.left + 4.0, field.top, field.right - field.left - 8.0, 22.0);
                let width = self.measure(shown, &self.fonts.body).min(text_area.right - text_area.left);
                if rename.fresh {
                    // Shown selected, since the first key replaces the whole name.
                    self.fill_round(rect(text_area.left, text_area.top + 3.0, width, 16.0), 2.0, faded(theme.accent, 0.35));
                }
                self.text(shown, &self.fonts.body, text_area, theme.text);
                if !rename.fresh && self.started.elapsed().as_millis() % 1060 < 530 {
                    self.fill(rect(text_area.left + width + 1.0, text_area.top + 3.0, 1.0, 16.0), theme.text);
                }
            }
            _ => self.text(&preset.name, &self.fonts.body, name_area, theme.text),
        }
        self.text(&preset.look.summary(), &self.fonts.small_line, rect(area.left + 42.0, area.top + 24.0, 200.0, 16.0), theme.muted);
    }

    unsafe fn draw_shortcuts(&self) {
        let theme = &self.theme;
        self.target.PushAxisAlignedClip(
            &rect(0.0, CONTENT_TOP - 6.0, WIDTH, LIST_BOTTOM - CONTENT_TOP + 6.0),
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        );
        let mut y = CONTENT_TOP - self.scroll;
        for line in Self::lines() {
            match &line {
                Line::Note => self.text(
                    "Click a box, then press any combination \u{2014} single keys included.",
                    &self.fonts.small,
                    rect(MARGIN, y, INNER, 34.0),
                    theme.muted,
                ),
                Line::Section(title) => self.text(title, &self.fonts.section, rect(MARGIN, y + 6.0, INNER, 20.0), theme.text),
                Line::Item(slot, label, scope) => {
                    let (field, clear) = Self::binding_box(y);
                    self.text(label, &self.fonts.body, rect(MARGIN, y + 3.0, 176.0, 22.0), theme.text);
                    self.text(scope, &self.fonts.small_line, rect(MARGIN, y + 23.0, 176.0, 16.0), theme.muted);
                    self.binding_field(*slot, field, clear);
                }
                Line::Fixed(label, keys, when) => {
                    let (field, _) = Self::binding_box(y);
                    self.text(label, &self.fonts.body, rect(MARGIN, y + 3.0, 176.0, 22.0), theme.text);
                    self.text(when, &self.fonts.small_line, rect(MARGIN, y + 23.0, 176.0, 16.0), theme.muted);
                    self.outline_round(field, 4.0, faded(theme.control_border, 0.6), 1.0);
                    self.text(keys, &self.fonts.body_centre, field, theme.muted);
                }
            }
            y += line_height(&line);
        }
        self.target.PopAxisAlignedClip();
        // Scroll thumb.
        let max = Self::max_scroll();
        if max > 0.0 {
            let view = LIST_BOTTOM - CONTENT_TOP;
            let length = view * view / (view + max);
            let top = CONTENT_TOP + (view - length) * self.scroll / max;
            self.fill_round(rect(WIDTH - 6.0, top, 3.0, length), 1.5, faded(theme.muted, 0.5));
        }
        self.fill(rect(0.0, LIST_BOTTOM, WIDTH, 1.0), theme.border);
        if !self.draw_status() {
            let hint = if self.capturing.is_some() {
                "Press the keys you want. Click anywhere else to cancel."
            } else {
                "Esc, Undo, Redo and Clear only listen while they can do something."
            };
            self.text(hint, &self.fonts.small_line, rect(MARGIN, LIST_BOTTOM + 8.0, INNER, 24.0), theme.muted);
        }
    }

    unsafe fn binding_field(&self, slot: Slot, field: D2D_RECT_F, clear: D2D_RECT_F) {
        let theme = &self.theme;
        let binding = settings::binding(slot);
        let capturing = self.capturing == Some(slot);
        let hovered = self.hover == Some(Widget::Binding(slot));
        let fill = if hovered || capturing { theme.control_hover } else { theme.control };
        self.fill_round(field, 4.0, fill);
        if capturing {
            self.outline_round(field, 4.0, theme.accent, 2.0);
            self.text("Press keys\u{2026}", &self.fonts.body_centre, field, theme.muted);
        } else if binding.is_set() {
            let failed = crate::hotkey_failed(slot);
            self.outline_round(field, 4.0, if failed { theme.danger } else { theme.control_border }, 1.0);
            self.text(&shortcut_name(binding), &self.fonts.body_centre, field, if failed { theme.danger } else { theme.text });
            if failed {
                self.icon(ICON_WARNING, rect(field.left + 4.0, field.top, 20.0, 30.0), theme.danger);
            }
        } else {
            self.outline_round(field, 4.0, theme.control_border, 1.0);
            self.text("Not set", &self.fonts.body_centre, field, theme.muted);
        }
        if binding.is_set() {
            if self.hover == Some(Widget::Unbind(slot)) {
                self.fill_round(clear, 4.0, theme.control_hover);
            }
            self.icon(ICON_CLOSE, clear, theme.muted);
        }
    }

    /// Returns whether there was a status to show.
    unsafe fn draw_status(&self) -> bool {
        let Some((text, at, error)) = &self.status else {
            return false;
        };
        if at.elapsed() > Duration::from_secs(5) {
            return false;
        }
        let color = if *error { self.theme.danger } else { self.theme.muted };
        let area = rect(MARGIN, LIST_BOTTOM + 8.0, INNER, 24.0);
        self.fill(area, self.theme.background);
        self.text(text, &self.fonts.small_line, area, color);
        true
    }

    unsafe fn draw_options(&self, settings: &Settings) {
        let theme = &self.theme;
        let named = match self.hover {
            Some(Widget::Cursor(index)) => index,
            _ => settings.cursor,
        };
        self.section(&format!("Cursor \u{b7} {}", CURSOR_STYLES[named].name), CONTENT_TOP);
        for (widget, area) in self.layout() {
            let hovered = self.hover == Some(widget);
            match widget {
                Widget::Cursor(index) => {
                    let selected = settings.cursor == index;
                    self.fill_round(area, 6.0, if hovered { theme.control_hover } else { theme.control });
                    if selected {
                        self.outline_round(area, 6.0, theme.accent, 2.0);
                    } else {
                        self.outline_round(area, 6.0, theme.control_border, 1.0);
                    }
                    let zoom = CURSOR_SWATCH / cursors::SIZE;
                    self.draw_cursor(index, settings.look.color, area.left, area.top, zoom);
                }
                Widget::Toggle(index) => self.toggle(index, area, settings, hovered),
                Widget::Reset => self.button(area, "Reset pen & options", hovered, theme.text),
                Widget::Quit => self.button(area, "Quit", hovered, theme.danger),
                _ => {}
            }
        }
        self.text(
            "Right-click passes a click through to the app underneath. The scroll wheel scrolls the page; Ctrl+wheel changes thickness.",
            &self.fonts.small,
            rect(MARGIN, TOGGLES_TOP + TOGGLES.len() as f32 * 52.0 + 6.0, INNER, 40.0),
            theme.muted,
        );
        if let Some((text, at, _)) = &self.status {
            if at.elapsed() < Duration::from_secs(5) {
                self.text(text, &self.fonts.small_line, rect(MARGIN, RESET.top - 28.0, INNER, 20.0), theme.muted);
            }
        }
    }

    // ---- Primitives -----------------------------------------------------------------------------

    /// Draws a cursor style's 48-unit square with its top-left at (x, y), scaled by `zoom`.
    unsafe fn draw_cursor(&self, style: usize, color: u32, x: f32, y: f32, zoom: f32) {
        let Some(svg) = &self.svg else { return };
        let key = (style, color);
        let mut documents = self.cursor_documents.borrow_mut();
        let document = match documents.iter().find(|(cached, _)| *cached == key) {
            Some((_, document)) => document.clone(),
            None => {
                let source = CURSOR_STYLES[style].svg(color);
                let Some(stream) = SHCreateMemStream(Some(source.as_bytes())) else {
                    return;
                };
                let size = D2D_SIZE_F {
                    width: cursors::SIZE,
                    height: cursors::SIZE,
                };
                let Ok(document) = svg.CreateSvgDocument(&stream, size) else {
                    return;
                };
                // Only the current colour's set is worth keeping.
                documents.retain(|((_, cached), _)| *cached == color);
                documents.push((key, document.clone()));
                document
            }
        };
        let mut previous = Matrix3x2::default();
        svg.GetTransform(&mut previous);
        svg.SetTransform(&Matrix3x2 {
            M11: zoom * previous.M11,
            M22: zoom * previous.M22,
            M31: previous.M31 + x * previous.M11,
            M32: previous.M32 + y * previous.M22,
            ..Default::default()
        });
        svg.DrawSvgDocument(&document);
        svg.SetTransform(&previous);
    }

    unsafe fn fill(&self, area: D2D_RECT_F, color: D2D1_COLOR_F) {
        self.brush.SetColor(&color);
        self.target.FillRectangle(&area, &self.brush);
    }

    unsafe fn fill_round(&self, area: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F) {
        self.brush.SetColor(&color);
        self.target.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: area,
                radiusX: radius,
                radiusY: radius,
            },
            &self.brush,
        );
    }

    unsafe fn outline_round(&self, area: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F, width: f32) {
        self.brush.SetColor(&color);
        let half = width / 2.0;
        self.target.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: area.left + half,
                    top: area.top + half,
                    right: area.right - half,
                    bottom: area.bottom - half,
                },
                radiusX: radius,
                radiusY: radius,
            },
            &self.brush,
            width,
            None,
        );
    }

    unsafe fn circle(&self, x: f32, y: f32, radius: f32, color: D2D1_COLOR_F) {
        self.brush.SetColor(&color);
        self.target.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: x, Y: y },
                radiusX: radius,
                radiusY: radius,
            },
            &self.brush,
        );
    }

    unsafe fn ring(&self, x: f32, y: f32, radius: f32, color: D2D1_COLOR_F, width: f32) {
        self.brush.SetColor(&color);
        self.target.DrawEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: x, Y: y },
                radiusX: radius,
                radiusY: radius,
            },
            &self.brush,
            width,
            None,
        );
    }

    unsafe fn text(&self, text: &str, format: &IDWriteTextFormat, area: D2D_RECT_F, color: D2D1_COLOR_F) {
        self.brush.SetColor(&color);
        self.target.DrawText(
            &wide(text),
            format,
            &area,
            &self.brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }

    unsafe fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        let measured = self
            .dwrite
            .CreateTextLayout(&wide(text), format, 1000.0, 100.0)
            .and_then(|layout| layout.GetMetrics(&mut metrics));
        if measured.is_ok() {
            metrics.widthIncludingTrailingWhitespace
        } else {
            0.0
        }
    }

    unsafe fn icon(&self, glyph: &str, area: D2D_RECT_F, color: D2D1_COLOR_F) {
        self.text(glyph, &self.fonts.icon, area, color);
    }

    unsafe fn section(&self, title: &str, y: f32) {
        self.text(title, &self.fonts.section, rect(MARGIN, y, INNER, 18.0), self.theme.text);
    }

    unsafe fn chip(&self, area: D2D_RECT_F, label: &str, selected: bool, hovered: bool, radius: f32) {
        let theme = &self.theme;
        if selected {
            self.fill_round(area, radius, theme.accent);
            self.text(label, &self.fonts.body_centre, area, theme.on_accent);
        } else {
            let fill = if hovered { theme.control_hover } else { theme.control };
            self.fill_round(area, radius, fill);
            self.outline_round(area, radius, theme.control_border, 1.0);
            self.text(label, &self.fonts.body_centre, area, theme.text);
        }
    }

    /// One part of a segmented control; only the outer ends are rounded.
    unsafe fn segment(&self, area: D2D_RECT_F, label: &str, selected: bool, hovered: bool, index: usize) {
        let theme = &self.theme;
        if index == 0 {
            let whole = rect(area.left, area.top, INNER, area.bottom - area.top);
            self.fill_round(whole, 6.0, theme.control);
            self.outline_round(whole, 6.0, theme.control_border, 1.0);
        }
        let inner = rect(area.left + 3.0, area.top + 3.0, area.right - area.left - 6.0, area.bottom - area.top - 6.0);
        if selected {
            self.fill_round(inner, 4.0, theme.accent);
            self.text(label, &self.fonts.body_centre, area, theme.on_accent);
        } else {
            if hovered {
                self.fill_round(inner, 4.0, theme.control_hover);
            }
            self.text(label, &self.fonts.body_centre, area, theme.text);
        }
    }

    unsafe fn button(&self, area: D2D_RECT_F, label: &str, hovered: bool, color: D2D1_COLOR_F) {
        let theme = &self.theme;
        let fill = if hovered { theme.control_hover } else { theme.control };
        self.fill_round(area, 4.0, fill);
        self.outline_round(area, 4.0, theme.control_border, 1.0);
        if !label.is_empty() {
            self.text(label, &self.fonts.body_centre, area, color);
        }
    }

    unsafe fn switch(&self, area: D2D_RECT_F, on: bool, hovered: bool) {
        let theme = &self.theme;
        let height = area.bottom - area.top;
        let radius = height / 2.0;
        let cy = area.top + radius;
        let knob = if hovered { radius * 0.62 } else { radius * 0.52 };
        if on {
            self.fill_round(area, radius, theme.accent);
            self.circle(area.right - radius, cy, knob, theme.on_accent);
        } else {
            self.fill_round(area, radius, theme.control);
            self.outline_round(area, radius, theme.muted, 1.0);
            self.circle(area.left + radius, cy, knob, theme.muted);
        }
    }

    unsafe fn swatch(&self, area: D2D_RECT_F, index: usize, color: u32, hovered: bool) {
        let theme = &self.theme;
        let cx = (area.left + area.right) / 2.0;
        let cy = (area.top + area.bottom) / 2.0;
        let custom = !SWATCHES.contains(&color);
        let (fill, selected) = match SWATCHES.get(index) {
            Some(&swatch) => (Some(swatch), swatch == color),
            None => (custom.then_some(color), custom),
        };
        if selected {
            self.ring(cx, cy, 13.0, theme.text, 2.0);
        } else if hovered {
            self.ring(cx, cy, 13.0, theme.muted, 1.0);
        }
        match fill {
            Some(swatch) => {
                self.circle(cx, cy, 9.5, hex(swatch));
                self.ring(cx, cy, 9.5, faded(theme.text, 0.15), 1.0);
            }
            None => {
                self.circle(cx, cy, 9.5, theme.control);
                self.ring(cx, cy, 9.5, theme.control_border, 1.0);
                self.icon(ICON_ADD, area, theme.muted);
            }
        }
    }

    unsafe fn slider(&self, index: usize, settings: &Settings, hovered: bool) {
        let theme = &self.theme;
        let slider = &SLIDERS[index];
        let y = SLIDER_TOP + index as f32 * 34.0;
        let enabled = Self::slider_enabled(index, settings);
        let alpha = if enabled { 1.0 } else { 0.4 };
        let value = (slider.get)(&settings.look);
        let (min, max) = slider.range;
        let k = (value - min) as f32 / (max - min) as f32;
        let cy = y + 17.0;
        self.text(slider.label, &self.fonts.body, rect(MARGIN, y, 104.0, 34.0), faded(theme.text, alpha));
        self.fill_round(rect(TRACK_X, cy - 2.0, TRACK_W, 4.0), 2.0, faded(theme.rail, alpha));
        let accent = if enabled { theme.accent } else { theme.rail };
        self.fill_round(rect(TRACK_X, cy - 2.0, TRACK_W * k, 4.0), 2.0, accent);
        let thumb = TRACK_X + TRACK_W * k;
        self.circle(thumb, cy, 10.0, theme.control);
        self.ring(thumb, cy, 10.0, theme.control_border, 1.0);
        let inner = if enabled && (hovered || self.dragging == Some(index)) { 7.0 } else { 5.0 };
        self.circle(thumb, cy, inner, accent);
        self.text(
            &(slider.show)(value),
            &self.fonts.body_right,
            rect(WIDTH - MARGIN - 56.0, y, 56.0, 34.0),
            faded(theme.muted, alpha.max(0.6)),
        );
    }

    unsafe fn toggle(&self, index: usize, area: D2D_RECT_F, settings: &Settings, hovered: bool) {
        let theme = &self.theme;
        let toggle = &TOGGLES[index];
        let (on, detail) = if index == AUTOSTART {
            let state = autostart_state();
            let detail = match state {
                Autostart::OffByUser => "Turned off in Task Manager; turn it back on there",
                Autostart::Policy(_) => "Set by your organisation",
                Autostart::Unknown => "Checking with Windows\u{2026}",
                Autostart::Failed => "Windows couldn't change this; try again",
                _ => toggle.detail,
            };
            (matches!(state, Autostart::On | Autostart::Policy(true)), detail)
        } else {
            ((toggle.get)(settings), toggle.detail)
        };
        if hovered {
            self.fill_round(area, 6.0, theme.control_hover);
        }
        let left = area.left + 8.0;
        self.text(toggle.title, &self.fonts.body, rect(left, area.top + 4.0, 300.0, 22.0), theme.text);
        self.text(detail, &self.fonts.small_line, rect(left, area.top + 25.0, 310.0, 18.0), theme.muted);
        self.switch(rect(area.right - 48.0, area.top + 14.0, 40.0, 20.0), on, hovered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;

    /// Writes every tab, light and dark, at 100 % and 150 %, to target/snapshots for review.
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn snapshot_panel() {
        snapshot::init();
        for (theme, light) in [("light", true), ("dark", false)] {
            for dpi in [96, 144] {
                for (tab, name) in [(PEN, "pen"), (PRESETS, "presets"), (SHORTCUTS, "shortcuts"), (OPTIONS, "options")] {
                    let scale = dpi as f32 / 96.0;
                    let (width, height) = ((WIDTH * scale) as u32, (HEIGHT * scale) as u32);
                    let canvas = snapshot::Canvas::new(width, height, dpi);
                    let mut panel = unsafe {
                        Panel::with_target(canvas.factory.clone(), canvas.target.clone(), dpi, Theme::new(light, Some(0x0078d4)))
                    }
                    .unwrap();
                    panel.tab = tab;
                    if tab == PEN {
                        for _ in 0..40 {
                            panel.preview.last -= Duration::from_millis(16);
                            panel.tick();
                        }
                    }
                    if tab == OPTIONS {
                        panel.hover = Some(Widget::Cursor(3));
                    }
                    if tab == SHORTCUTS {
                        panel.scroll = Panel::max_scroll();
                    }
                    unsafe { panel.draw() };
                    canvas.save(&format!("panel-{name}-{theme}-{dpi}.png"));
                }
            }
        }
    }
}
