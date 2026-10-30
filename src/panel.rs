//! The settings panel: drawn with Direct2D in the Windows 11 style, following the system's
//! light/dark theme and accent colour. One page that scrolls under a fixed header: presets and the
//! pen first, then the cursor and shortcuts, and the settings that are set once under More.

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
    VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_UP,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::settings::{
    self, Autostart, Binding, Command, Look, Preset, Settings, Slot, COMMANDS, MAX_NAME, MAX_PRESETS,
    STYLE_GLOW, STYLE_NAMES, STYLE_SOLID,
};
use crate::cursors::{self, STYLES as CURSOR_STYLES};
use crate::stroke::{self, rgba, Point, Scratch, Stroke};
use crate::store::{self, Licence};
use crate::ui::shortcut_name;

pub const WIDTH: f32 = 420.0;
pub const HEIGHT: f32 = 636.0;
const MARGIN: f32 = 20.0;
const INNER: f32 = WIDTH - 2.0 * MARGIN;
/// The header stays put; everything below it scrolls.
const VIEW_TOP: f32 = 72.0;
/// Along the bottom while there is something to say: a message, or what to do while recording a
/// shortcut.
const STRIP: f32 = 34.0;
/// How far the wheel and the arrow keys move the page.
const STEP: f32 = 48.0;
const CHIP_HEIGHT: f32 = 32.0;
const CHIP_GAP: f32 = 8.0;
const PREVIEW_HEIGHT: f32 = 130.0;
/// How long the pointer rests on an icon before its tooltip shows.
const TIP_DELAY: Duration = Duration::from_millis(400);

pub const SWATCHES: [u32; 7] = [0xff2a2a, 0xff8a00, 0xffd400, 0x2ecc40, 0x1e90ff, 0xd62aff, 0xff4fa8];
const STYLES: [u32; 2] = [STYLE_SOLID, STYLE_GLOW];

const ICON_ADD: &str = "\u{e710}";
const ICON_CLOSE: &str = "\u{e711}";
const ICON_WARNING: &str = "\u{e7ba}";
const ICON_DOWN: &str = "\u{e70d}";
const ICON_UP: &str = "\u{e70e}";

struct SliderDef {
    label: &'static str,
    range: (i32, i32),
    get: fn(&Look) -> i32,
    set: fn(&mut Look, i32),
    show: fn(i32) -> String,
}

const FADE: usize = 2;
/// The sliders always in view, and the ones under More.
const MAIN_SLIDERS: [usize; 3] = [0, FADE, 4];
const MORE_SLIDERS: [usize; 2] = [1, 3];
const SLIDER_STEP: f32 = 34.0;

fn smoothing_name(value: i32) -> String {
    match value {
        0 => "Off",
        1..=2 => "Low",
        3..=5 => "Medium",
        _ => "High",
    }
    .into()
}

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
        show: smoothing_name,
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

/// Keep strokes sits with the pen; the rest are under More.
const KEEP_STROKES: usize = 0;
const AUTOSTART: usize = 5;
const TOGGLE_STEP: f32 = 48.0;

const TOGGLES: [ToggleDef; 6] = [
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
        title: "Show where the pointer is",
        detail: "A ring closes in on the cursor as the laser turns on",
        get: |s| s.pulse,
        set: |s, v| s.pulse = v,
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

const TIP: &str = "Click a box and press the keys you want. While drawing, hold Alt for a level line or \
Shift+Alt for an upright one, and hold Ctrl as you let go to keep a shape freehand. Right-click \
clicks through to the app underneath; Ctrl+wheel changes thickness.";
const TIP_HEIGHT: f32 = 66.0;
const MORE_HEIGHT: f32 = 44.0;

const POWER: D2D_RECT_F = rect(WIDTH - MARGIN - 44.0, 20.0, 44.0, 22.0);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Widget {
    Power,
    Chip(usize),
    NewPreset,
    Swatch(usize),
    /// The colour picker's card, and the hue and shade bars and Done button on it.
    Picker,
    Hue,
    Shade,
    PickerDone,
    Style(usize),
    Slider(usize),
    Toggle(usize),
    Cursor(usize),
    Binding(Slot),
    Unbind(Slot),
    More,
    Link(usize),
    Reset,
    Quit,
    /// The scroll bar's thumb.
    Thumb,
}

/// What a held mouse button is dragging.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Slider(usize),
    Hue,
    Shade,
    /// The thumb, held this far below its top.
    Thumb(f32),
}

/// The links on the version line. Until the Store build is bought, the first one buys it.
fn links() -> [(String, String); 2] {
    let first = match (store::licence(), settings::package_family()) {
        (Licence::Trial(days), _) => {
            let plural = if days == 1 { "" } else { "s" };
            (format!("Buy \u{b7} {days} day{plural} of trial left"), store::product_page())
        }
        (Licence::Unlicensed { .. }, _) => ("Buy in the Store".into(), store::product_page()),
        (Licence::Full | Licence::Unknown, Some(family)) => ("Rate in the Store".into(), format!("ms-windows-store://review/?PFN={family}")),
        (Licence::Full | Licence::Unknown, None) => ("Release notes".into(), "https://github.com/ayricky/laser-pointer/releases".into()),
    };
    [first, ("Report a problem".into(), "https://github.com/ayricky/laser-pointer/issues/new".into())]
}

pub enum Action {
    None,
    Redraw,
    SettingsChanged,
    Toggle(bool),
    /// Something the tray menu and shortcuts do too, done the same way.
    Run(Command),
    /// Opens a web page or the Store.
    Open(String),
    /// A preset was right-clicked: show what can be done to it.
    PresetMenu(usize),
    Quit,
    Close,
}

/// What a preset's menu can do to it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PresetChoice {
    Update,
    Rename,
    Shortcut,
    Unbind,
    Delete,
}

const fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

/// The same rectangle `by` lower.
fn down(area: D2D_RECT_F, by: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        top: area.top + by,
        bottom: area.bottom + by,
        ..area
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

/// Hue (0 to 1 round the wheel), saturation and lightness, as the colour picker holds them.
type Hsl = (f32, f32, f32);

/// A colour from its hue, saturation and lightness.
fn from_hsl((hue, saturation, light): Hsl) -> u32 {
    let chroma = (1.0 - (2.0 * light - 1.0).abs()).max(0.0) * saturation.clamp(0.0, 1.0);
    let h = hue.rem_euclid(1.0) * 6.0;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = light - chroma / 2.0;
    let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// The hue, saturation and lightness of a colour, as the picker places them.
fn to_hsl(rgb: u32) -> Hsl {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    let (r, g, b) = (channel(16), channel(8), channel(0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let light = (max + min) / 2.0;
    let delta = max - min;
    if delta < 1e-6 {
        return (0.0, 0.0, light);
    }
    let saturation = delta / (1.0 - (2.0 * light - 1.0).abs()).max(1e-6);
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (hue / 6.0, saturation.min(1.0), light)
}

/// How light the picker's shade bar runs, from black to white.
const SHADES: (f32, f32) = (0.0, 1.0);

/// The colour picker: a card under the swatches, over the style and sliders, and what is on it.
struct Picker {
    card: D2D_RECT_F,
    hue: D2D_RECT_F,
    shade: D2D_RECT_F,
    done: D2D_RECT_F,
}

impl Picker {
    fn under(swatches: D2D_RECT_F) -> Self {
        let card = rect(MARGIN, swatches.bottom + 12.0, INNER, 138.0);
        Self {
            card,
            hue: rect(card.left + 16.0, card.top + 46.0, INNER - 32.0, 12.0),
            shade: rect(card.left + 16.0, card.top + 78.0, INNER - 32.0, 12.0),
            done: rect(card.right - 16.0 - 76.0, card.top + 100.0, 76.0, 28.0),
        }
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
    /// Measured from the preview's top left, so they move with it as the page scrolls.
    points: Vec<Point>,
    t: f32,
    last: Instant,
    scratch: Scratch,
}

impl Preview {
    /// Where the pretend hand is at `t`, from the preview's top left.
    fn position(t: f32) -> (f32, f32) {
        let cx = INNER / 2.0;
        let cy = PREVIEW_HEIGHT / 2.0 + 6.0;
        (cx + 130.0 * t.sin(), cy + 36.0 * (2.0 * t).sin())
    }
}

struct Rename {
    index: usize,
    text: String,
    /// Typing replaces the suggested name rather than appending to it.
    fresh: bool,
}

/// A line in the shortcuts list is this tall.
const ROW: f32 = 44.0;

/// The cursor picker, between the pen and the shortcuts.
const CURSOR_SWATCH: f32 = 40.0;

/// Where each part of the page sits, measured from the top of the part that scrolls.
struct Page {
    chips: Vec<(Widget, D2D_RECT_F)>,
    preview: D2D_RECT_F,
    colour: f32,
    style: f32,
    sliders: f32,
    keep: f32,
    cursor: f32,
    keys: f32,
    /// Each shortcut line and its top.
    rows: Vec<(Slot, f32)>,
    tip: f32,
    more: f32,
    /// Where More's sliders start, when it is open.
    extra: f32,
    version: f32,
    buttons: f32,
    height: f32,
}

pub struct Panel {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    target: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
    fonts: Fonts,
    pub theme: Theme,
    pub scale: f32,
    hover: Option<Widget>,
    /// Where the pointer was last seen, to find what is under it once the page scrolls.
    pointer: Option<(f32, f32)>,
    /// When the pointer came to rest on what it's over, for tooltips, and whether that one's
    /// tooltip has been drawn.
    hover_since: Instant,
    tip_shown: bool,
    dragging: Option<Drag>,
    /// The colour picker is open, at this hue, saturation and lightness.
    picking: Option<Hsl>,
    capturing: Option<Slot>,
    renaming: Option<Rename>,
    status: Option<(String, Instant, bool)>,
    scroll: f32,
    /// More is open.
    more: bool,
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

/// What a shortcut line says: what the shortcut does, and when it listens.
fn row_label(slot: Slot) -> (String, &'static str) {
    match slot {
        Slot::Command(command) => (command.label().into(), command.scope()),
        Slot::Preset(index) => {
            let name = settings::presets().get(index).map(|preset| preset.name.clone()).unwrap_or_default();
            (name, "switches to this preset")
        }
    }
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
        // The trial's days left are counted afresh each time the panel opens.
        store::check(crate::overlay());
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

    /// Moved to a monitor at another scale: draw for it, the window now `width` by `height`.
    pub unsafe fn resize(&mut self, dpi: u32, width: u32, height: u32) -> Result<()> {
        let Some((hwnd, ..)) = self.window else { return Ok(()) };
        self.window = Some((hwnd, dpi, width, height));
        self.scale = dpi as f32 / 96.0;
        self.rebuild_target()
    }

    /// The GPU was reset: draw into a fresh target, keeping everything else about the panel.
    unsafe fn rebuild_target(&mut self) -> Result<()> {
        let Some((hwnd, dpi, width, height)) = self.window else {
            return Ok(());
        };
        self.preview.scratch.forget_device();
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
            hover: None,
            pointer: None,
            hover_since: Instant::now(),
            tip_shown: false,
            dragging: None,
            picking: None,
            capturing: None,
            renaming: None,
            status: None,
            scroll: 0.0,
            more: false,
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

    /// Recording a shortcut, typing a preset's name, or open on something Esc closes, so the
    /// app's own shortcuts must let go.
    pub fn wants_keys(&self) -> bool {
        self.capturing.is_some() || self.renaming.is_some() || self.picking.is_some()
    }

    fn status_showing(&self) -> bool {
        self.status.as_ref().is_some_and(|(_, at, _)| at.elapsed() < Duration::from_secs(5))
    }

    /// The strip along the bottom is up, over the end of the page.
    fn strip_showing(&self) -> bool {
        self.capturing.is_some() || self.status_showing()
    }

    /// Where the page can be seen, below the header and above the strip.
    fn view_bottom(&self) -> f32 {
        if self.strip_showing() {
            HEIGHT - STRIP
        } else {
            HEIGHT
        }
    }

    /// How far down the page is shown: page positions plus this are panel positions.
    fn offset(&self) -> f32 {
        VIEW_TOP - self.scroll
    }

    fn max_scroll(&self) -> f32 {
        let page = unsafe { self.page() };
        (page.height - (HEIGHT - VIEW_TOP)).max(0.0)
    }

    /// Moves the page by `delta`, down for positive, and finds what the pointer is over now.
    fn scroll_by(&mut self, delta: f32) -> Action {
        let to = (self.scroll + delta).clamp(0.0, self.max_scroll());
        if to == self.scroll {
            return Action::None;
        }
        self.scroll = to;
        self.find_hover();
        Action::Redraw
    }

    /// Scrolls just enough to show the stretch of page from `top` to `bottom`.
    fn reveal(&mut self, top: f32, bottom: f32) {
        let view = self.view_bottom() - VIEW_TOP;
        if top < self.scroll + 8.0 {
            self.scroll = top - 8.0;
        } else if bottom > self.scroll + view - 8.0 {
            self.scroll = bottom - view + 8.0;
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        self.find_hover();
    }

    fn find_hover(&mut self) {
        let hover = self.pointer.and_then(|(x, y)| self.hit(x, y));
        if hover != self.hover {
            self.hover = hover;
            self.hover_since = Instant::now();
            self.tip_shown = false;
        }
    }

    /// Advances the preview's pretend hand along a figure of eight. Returns whether anything on
    /// screen is moving, so an idle panel isn't repainted sixty times a second.
    pub fn tick(&mut self) -> bool {
        self.scroll = self.scroll.min(self.max_scroll());
        let status_showing = self
            .status
            .as_ref()
            .is_some_and(|(_, at, _)| at.elapsed() < Duration::from_millis(5100));
        // A tooltip is due to show.
        let tip_due = self.tip().is_some() && !self.tip_shown && self.hover.and_then(|hover| self.area_of(hover)).is_some();
        let preview = down(unsafe { self.page() }.preview, self.offset());
        if preview.bottom <= VIEW_TOP || preview.top >= HEIGHT {
            return self.renaming.is_some() || status_showing || tip_due;
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

    /// Lays the page out from the top, each part under the last. The preset chips wrap, and the
    /// shortcuts list and More grow with what they hold.
    unsafe fn page(&self) -> Page {
        let presets = settings::presets();
        let mut chips = Vec::new();
        let (mut x, mut y) = (MARGIN, 16.0);
        let mut place = |widget: Widget, width: f32| {
            if x > MARGIN && x + width > WIDTH - MARGIN {
                x = MARGIN;
                y += CHIP_HEIGHT + CHIP_GAP;
            }
            chips.push((widget, rect(x, y, width, CHIP_HEIGHT)));
            x += width + CHIP_GAP;
        };
        for (index, preset) in presets.iter().enumerate() {
            let name = match &self.renaming {
                Some(rename) if rename.index == index => rename.text.as_str(),
                _ => preset.name.as_str(),
            };
            let width = self.measure(name, &self.fonts.body).max(24.0) + 46.0;
            place(Widget::Chip(index), width.min(INNER));
        }
        if presets.len() < MAX_PRESETS {
            place(Widget::NewPreset, CHIP_HEIGHT + 4.0);
        }
        let mut y = chips.last().map_or(0.0, |(_, area)| area.bottom) + 16.0;
        let mut take = |height: f32| {
            let top = y;
            y += height;
            top
        };
        let preview = rect(MARGIN, take(PREVIEW_HEIGHT + 18.0), INNER, PREVIEW_HEIGHT);
        let colour = take(22.0 + 28.0 + 16.0);
        let style = take(22.0 + 30.0 + 10.0);
        let sliders = take(MAIN_SLIDERS.len() as f32 * SLIDER_STEP + 4.0);
        let keep = take(44.0 + 18.0);
        let cursor = take(26.0 + CURSOR_SWATCH + 24.0);
        let keys = take(28.0);
        // A preset's line shows once it has a shortcut, or while it gets one.
        let preset_rows = presets
            .iter()
            .enumerate()
            .filter(|(index, preset)| preset.key.is_set() || self.capturing == Some(Slot::Preset(*index)))
            .map(|(index, _)| Slot::Preset(index));
        let slots: Vec<Slot> = COMMANDS.iter().map(|&command| Slot::Command(command)).chain(preset_rows).collect();
        let rows = slots.into_iter().map(|slot| (slot, take(ROW))).collect();
        take(6.0);
        let tip = take(TIP_HEIGHT + 12.0);
        let more = take(MORE_HEIGHT + 4.0);
        let extra = take(if self.more {
            MORE_SLIDERS.len() as f32 * SLIDER_STEP + 6.0 + (TOGGLES.len() - 1) as f32 * TOGGLE_STEP + 4.0
        } else {
            0.0
        });
        take(14.0);
        let version = take(20.0 + 14.0);
        let buttons = take(32.0 + 20.0);
        Page {
            chips,
            preview,
            colour,
            style,
            sliders,
            keep,
            cursor,
            keys,
            rows,
            tip,
            more,
            extra,
            version,
            buttons,
            height: y,
        }
    }

    fn binding_box(y: f32) -> (D2D_RECT_F, D2D_RECT_F) {
        (
            rect(WIDTH - MARGIN - 36.0 - 150.0, y + 7.0, 150.0, 30.0),
            rect(WIDTH - MARGIN - 30.0, y + 7.0, 30.0, 30.0),
        )
    }

    fn swatch_rects(top: f32) -> Vec<D2D_RECT_F> {
        let slots = SWATCHES.len() as f32 + 1.0;
        let gap = (INNER - slots * 28.0) / (slots - 1.0);
        (0..=SWATCHES.len()).map(|index| rect(MARGIN + index as f32 * (28.0 + gap), top, 28.0, 28.0)).collect()
    }

    /// The scroll bar's thumb, when the page is longer than the panel.
    fn thumb(&self) -> Option<D2D_RECT_F> {
        let max = self.max_scroll();
        if max <= 0.0 {
            return None;
        }
        let view = HEIGHT - VIEW_TOP;
        let length = (view * view / (view + max)).max(32.0);
        let top = VIEW_TOP + (view - length) * self.scroll / max;
        Some(rect(WIDTH - 12.0, top, 12.0, length))
    }

    /// Every widget that can be seen, where it is in the panel. The picker comes first, so it
    /// takes the clicks meant for it over what lies beneath.
    fn layout(&self) -> Vec<(Widget, D2D_RECT_F)> {
        let page = unsafe { self.page() };
        let by = self.offset();
        let mut content = Vec::new();
        let swatches = Self::swatch_rects(page.colour + 22.0 + by);
        if self.picking.is_some() {
            let picker = Picker::under(swatches[0]);
            // The bars take clicks a little above and below, so they're easy to catch.
            let grab = |bar: D2D_RECT_F| rect(bar.left - 10.0, bar.top - 10.0, bar.right - bar.left + 20.0, bar.bottom - bar.top + 20.0);
            content.push((Widget::Hue, grab(picker.hue)));
            content.push((Widget::Shade, grab(picker.shade)));
            content.push((Widget::PickerDone, picker.done));
            content.push((Widget::Picker, picker.card));
        }
        content.extend(page.chips.iter().map(|&(widget, area)| (widget, down(area, by))));
        content.extend(swatches.into_iter().enumerate().map(|(index, area)| (Widget::Swatch(index), area)));
        let half = INNER / STYLES.len() as f32;
        for index in 0..STYLES.len() {
            content.push((Widget::Style(index), rect(MARGIN + index as f32 * half, page.style + 22.0 + by, half, 30.0)));
        }
        let slider = |top: f32| rect(TRACK_X - 10.0, top, TRACK_W + 20.0, SLIDER_STEP);
        for (k, &index) in MAIN_SLIDERS.iter().enumerate() {
            content.push((Widget::Slider(index), slider(page.sliders + k as f32 * SLIDER_STEP + by)));
        }
        let toggle = |top: f32| rect(MARGIN - 8.0, top, INNER + 16.0, 44.0);
        content.push((Widget::Toggle(KEEP_STROKES), toggle(page.keep + by)));
        let count = CURSOR_STYLES.len() as f32;
        let gap = (INNER - count * CURSOR_SWATCH) / (count - 1.0);
        for index in 0..CURSOR_STYLES.len() {
            let x = MARGIN + index as f32 * (CURSOR_SWATCH + gap);
            content.push((Widget::Cursor(index), rect(x, page.cursor + 26.0 + by, CURSOR_SWATCH, CURSOR_SWATCH)));
        }
        for &(slot, top) in &page.rows {
            let (field, clear) = Self::binding_box(top + by);
            content.push((Widget::Binding(slot), field));
            if settings::binding(slot).is_set() {
                content.push((Widget::Unbind(slot), clear));
            }
        }
        content.push((Widget::More, rect(MARGIN - 8.0, page.more + by, INNER + 16.0, MORE_HEIGHT)));
        if self.more {
            for (k, &index) in MORE_SLIDERS.iter().enumerate() {
                content.push((Widget::Slider(index), slider(page.extra + k as f32 * SLIDER_STEP + by)));
            }
            let toggles = page.extra + MORE_SLIDERS.len() as f32 * SLIDER_STEP + 6.0;
            for index in 1..TOGGLES.len() {
                content.push((Widget::Toggle(index), toggle(toggles + (index - 1) as f32 * TOGGLE_STEP + by)));
            }
        }
        for (index, area) in self.link_rects(page.version + by).into_iter().enumerate() {
            content.push((Widget::Link(index), area));
        }
        content.push((Widget::Reset, rect(MARGIN, page.buttons + by, 160.0, 32.0)));
        content.push((Widget::Quit, rect(WIDTH - MARGIN - 100.0, page.buttons + by, 100.0, 32.0)));

        let bottom = self.view_bottom();
        let mut widgets = vec![(Widget::Power, POWER)];
        widgets.extend(self.thumb().map(|thumb| (Widget::Thumb, thumb)));
        widgets.extend(content.into_iter().filter(|(_, area)| area.bottom > VIEW_TOP && area.top < bottom));
        widgets
    }

    /// The version line: each link's area after the version.
    fn link_rects(&self, top: f32) -> Vec<D2D_RECT_F> {
        let mut x = MARGIN + unsafe { self.measure(&Self::version(), &self.fonts.small_line) } + 14.0;
        links()
            .iter()
            .map(|(label, _)| {
                let width = unsafe { self.measure(label, &self.fonts.small_line) };
                let area = rect(x, top, width, 20.0);
                x += width + 14.0;
                area
            })
            .collect()
    }

    fn version() -> String {
        format!("Laser Pointer {}", env!("CARGO_PKG_VERSION"))
    }

    /// What is under the pointer. Only the power switch is in the header, and nothing on the
    /// page under the strip takes clicks.
    fn hit(&self, x: f32, y: f32) -> Option<Widget> {
        let on_page = y >= VIEW_TOP && y < self.view_bottom();
        self.layout()
            .into_iter()
            .find(|(widget, area)| (on_page || *widget == Widget::Power) && contains(area, x, y))
            .map(|(widget, _)| widget)
    }

    // ---- Input ----------------------------------------------------------------------------------

    pub fn mouse_move(&mut self, x: f32, y: f32) -> Action {
        self.pointer = Some((x, y));
        if let Some(drag) = self.dragging {
            return self.drag(drag, x, y);
        }
        let hover = self.hit(x, y);
        if hover == self.hover {
            return Action::None;
        }
        self.hover = hover;
        self.hover_since = Instant::now();
        self.tip_shown = false;
        Action::Redraw
    }

    pub fn mouse_leave(&mut self) {
        self.hover = None;
        self.pointer = None;
    }

    pub fn scroll(&mut self, delta: i16) -> Action {
        self.scroll_by(-f32::from(delta) / 120.0 * STEP)
    }

    pub fn mouse_down(&mut self, x: f32, y: f32) -> Action {
        let widget = self.hit(x, y);
        let renaming = self.renaming.as_ref().map(|rename| rename.index);
        if renaming.is_some() && widget != renaming.map(Widget::Chip) {
            self.finish_rename(true);
        }
        if self.capturing.is_some() && !matches!(widget, Some(Widget::Binding(_))) {
            self.capturing = None;
        }
        let on_picker = matches!(widget, Some(Widget::Picker | Widget::Hue | Widget::Shade | Widget::Swatch(_)));
        if self.picking.is_some() && !on_picker {
            self.picking = None;
        }
        let Some(widget) = widget else {
            return Action::Redraw;
        };
        let mut current = settings::get();
        match widget {
            Widget::Power => return Action::Toggle(!crate::laser_wanted()),
            Widget::Chip(index) => {
                if renaming == Some(index) {
                    return Action::None;
                }
                settings::apply_preset(index);
                settings::save();
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
            Widget::Swatch(index) if index < SWATCHES.len() => {
                self.picking = None;
                current.look.color = SWATCHES[index];
            }
            Widget::Swatch(_) => {
                self.picking = match self.picking {
                    Some(_) => None,
                    None => Some(to_hsl(current.look.color)),
                };
                if self.picking.is_some() {
                    let page = unsafe { self.page() };
                    let card = Picker::under(Self::swatch_rects(page.colour + 22.0)[0]).card;
                    self.reveal(page.colour, card.bottom);
                }
                return Action::Redraw;
            }
            Widget::Hue => {
                self.dragging = Some(Drag::Hue);
                return self.drag(Drag::Hue, x, y);
            }
            Widget::Shade => {
                self.dragging = Some(Drag::Shade);
                return self.drag(Drag::Shade, x, y);
            }
            Widget::PickerDone => {
                self.picking = None;
                return Action::Redraw;
            }
            Widget::Picker => return Action::None,
            Widget::Thumb => {
                let top = self.thumb().map_or(y, |thumb| thumb.top);
                self.dragging = Some(Drag::Thumb(y - top));
                return Action::Redraw;
            }
            Widget::More => {
                self.more = !self.more;
                if self.more {
                    // Brought up to the top, so what it opened is in view.
                    let top = unsafe { self.page() }.more;
                    self.scroll = (top - 8.0).clamp(0.0, self.max_scroll());
                } else {
                    self.scroll = self.scroll.min(self.max_scroll());
                }
                self.find_hover();
                return Action::Redraw;
            }
            Widget::Link(index) => return Action::Open(links()[index].1.clone()),
            Widget::Style(index) => current.look.style = STYLES[index],
            Widget::Cursor(index) => current.cursor = index,
            Widget::Slider(index) => {
                if !Self::slider_enabled(index, &current) {
                    return Action::None;
                }
                self.dragging = Some(Drag::Slider(index));
                return self.drag(Drag::Slider(index), x, y);
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
                let (keys, hinted) = (current.keys, current.hinted);
                current = Settings::default();
                current.keys = keys;
                current.hinted = hinted;
                self.say("Pen and options reset; shortcuts and presets kept", false);
            }
            Widget::Quit => return Action::Quit,
        }
        settings::set(current);
        settings::save();
        Action::SettingsChanged
    }

    /// A right-click on a preset asks for its menu; anywhere else it does nothing.
    pub fn right_click(&mut self, x: f32, y: f32) -> Action {
        let Some(Widget::Chip(index)) = self.hit(x, y) else {
            return Action::None;
        };
        self.finish_rename(true);
        self.capturing = None;
        self.picking = None;
        Action::PresetMenu(index)
    }

    /// Does what was picked from a preset's menu.
    pub fn preset_choice(&mut self, index: usize, choice: PresetChoice) -> Action {
        let mut list = settings::presets();
        let Some(preset) = list.get(index).cloned() else {
            return Action::None;
        };
        match choice {
            PresetChoice::Update => {
                list[index].look = settings::get().look;
                settings::set_presets(list);
                settings::save();
                self.say(format!("Updated \u{201c}{}\u{201d} to the current pen", preset.name), false);
                Action::SettingsChanged
            }
            PresetChoice::Rename => {
                self.renaming = Some(Rename {
                    index,
                    text: preset.name,
                    fresh: true,
                });
                Action::Redraw
            }
            PresetChoice::Shortcut => {
                let slot = Slot::Preset(index);
                self.capturing = Some(slot);
                self.status = None;
                if let Some(&(_, top)) = unsafe { self.page() }.rows.iter().find(|(row, _)| *row == slot) {
                    self.reveal(top, top + ROW);
                }
                Action::Redraw
            }
            PresetChoice::Unbind => {
                settings::set_binding(Slot::Preset(index), Binding::NONE);
                Action::SettingsChanged
            }
            PresetChoice::Delete => {
                list.remove(index);
                settings::set_presets(list);
                settings::save();
                // The chips after it moved along, so something else may be under the pointer.
                self.find_hover();
                self.say(format!("Deleted \u{201c}{}\u{201d}", preset.name), false);
                Action::SettingsChanged
            }
        }
    }

    pub fn mouse_up(&mut self) -> Action {
        match self.dragging.take() {
            Some(Drag::Thumb(_)) => Action::Redraw,
            Some(_) => {
                settings::save();
                Action::SettingsChanged
            }
            None => Action::None,
        }
    }

    fn slider_enabled(index: usize, settings: &Settings) -> bool {
        match index {
            FADE => !settings.look.ink,
            _ => true,
        }
    }

    /// Follows the mouse with whatever it's dragging. Settings change as it moves, and are saved
    /// and passed on once it lets go.
    fn drag(&mut self, drag: Drag, x: f32, y: f32) -> Action {
        let mut current = settings::get();
        match drag {
            Drag::Slider(index) => {
                let slider = &SLIDERS[index];
                let (min, max) = slider.range;
                let k = ((x - TRACK_X) / TRACK_W).clamp(0.0, 1.0);
                let value = min + (k * (max - min) as f32).round() as i32;
                (slider.set)(&mut current.look, value);
            }
            Drag::Hue | Drag::Shade => {
                let Some(picked) = &mut self.picking else { return Action::None };
                let bar = Picker::under(rect(0.0, 0.0, 0.0, 0.0)).hue;
                let k = ((x - bar.left) / (bar.right - bar.left)).clamp(0.0, 1.0);
                if drag == Drag::Hue {
                    picked.0 = k;
                    // A pure grey has no hue to move; choosing one makes it a colour. A muted
                    // colour keeps its own saturation.
                    if picked.1 < 0.005 {
                        picked.1 = 1.0;
                    }
                } else {
                    picked.2 = SHADES.0 + (SHADES.1 - SHADES.0) * k;
                }
                current.look.color = from_hsl(*picked);
            }
            Drag::Thumb(grab) => {
                let (Some(thumb), max) = (self.thumb(), self.max_scroll()) else { return Action::None };
                let travel = HEIGHT - VIEW_TOP - (thumb.bottom - thumb.top);
                let k = ((y - grab - VIEW_TOP) / travel.max(1.0)).clamp(0.0, 1.0);
                return self.scroll_by(k * max - self.scroll);
            }
        }
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
        if is(VK_ESCAPE) && self.picking.is_some() {
            self.picking = None;
            return Action::Redraw;
        }
        if is(VK_ESCAPE) {
            return Action::Close;
        }
        let page = HEIGHT - VIEW_TOP - STEP;
        let delta = match VIRTUAL_KEY(vk as u16) {
            VK_UP => -STEP,
            VK_DOWN => STEP,
            VK_PRIOR => -page,
            VK_NEXT => page,
            VK_HOME => -f32::INFINITY,
            VK_END => f32::INFINITY,
            _ => return Action::None,
        };
        self.scroll_by(delta)
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
        self.target.PushAxisAlignedClip(&rect(0.0, VIEW_TOP, WIDTH, HEIGHT - VIEW_TOP), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        self.draw_page(&settings);
        self.target.PopAxisAlignedClip();
        self.draw_header(&settings);
        self.draw_scroll_bar();
        self.draw_strip();
        self.tip_shown = self.draw_tip();
        let drawn = self.target.EndDraw(None, None);
        self.preview.scratch.finish_frame();
        drawn
    }

    /// What the hovered widget does, for one that has no label of its own.
    fn tip(&self) -> Option<String> {
        Some(match self.hover? {
            Widget::Chip(index) if self.renaming.is_none() => {
                let preset = settings::presets().get(index)?.clone();
                format!("{} \u{b7} right-click for more", preset.look.summary())
            }
            Widget::NewPreset => "New preset from the current pen".into(),
            Widget::Unbind(_) => "Remove shortcut".into(),
            Widget::Swatch(index) if index == SWATCHES.len() => "Custom colour".into(),
            Widget::Power => "Turn the laser on or off".into(),
            _ => return None,
        })
    }

    /// A tooltip under the hovered widget once the pointer has rested on it, kept inside the
    /// panel. Returns whether it drew one.
    unsafe fn draw_tip(&self) -> bool {
        let (Some(text), Some(anchor)) = (self.tip(), self.hover.and_then(|hover| self.area_of(hover))) else {
            return false;
        };
        if self.hover_since.elapsed() < TIP_DELAY {
            return false;
        }
        let theme = &self.theme;
        let width = self.measure(&text, &self.fonts.small_line) + 18.0;
        let left = (anchor.right - width).clamp(8.0, WIDTH - 8.0 - width);
        let mut top = anchor.bottom + 6.0;
        if top + 26.0 > HEIGHT - 8.0 {
            top = anchor.top - 6.0 - 26.0;
        }
        let bubble = rect(left, top, width, 26.0);
        self.fill_round(rect(left, top + 1.0, width, 26.0), 4.0, faded(hex(0x000000), 0.18));
        self.fill_round(bubble, 4.0, theme.control);
        self.outline_round(bubble, 4.0, theme.control_border, 1.0);
        self.text(&text, &self.fonts.small_line, rect(left + 9.0, top, width - 18.0, 26.0), theme.text);
        true
    }

    fn area_of(&self, widget: Widget) -> Option<D2D_RECT_F> {
        self.layout().into_iter().find(|(shown, _)| *shown == widget).map(|(_, area)| area)
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
        // The dot and the switch both say whether it's on, so there's no On or Off beside them.
        let mut hints = Vec::new();
        if settings.key(Command::Toggle).is_set() {
            hints.push(format!("{} to toggle", shortcut_name(settings.key(Command::Toggle))));
        }
        if settings.key(Command::Off).is_set() {
            hints.push(format!("{} to stop", shortcut_name(settings.key(Command::Off))));
        }
        let hint = if hints.is_empty() {
            "No shortcuts set; add them under Shortcuts".to_string()
        } else {
            hints.join("  \u{b7}  ")
        };
        self.text(&hint, &self.fonts.small_line, rect(MARGIN, 42.0, 300.0, 18.0), theme.muted);
        self.switch(POWER, on, self.hover == Some(Widget::Power));
        self.fill(rect(0.0, VIEW_TOP - 1.0, WIDTH, 1.0), theme.border);
    }

    unsafe fn draw_page(&mut self, settings: &Settings) {
        // The colour changed some other way while the picker was open, a preset or the tray say.
        if self.picking.is_some_and(|picked| self.dragging.is_none() && from_hsl(picked) != settings.look.color) {
            self.picking = Some(to_hsl(settings.look.color));
        }
        let page = self.page();
        let by = self.offset();
        let preview = down(page.preview, by);
        if preview.bottom > VIEW_TOP && preview.top < HEIGHT {
            self.draw_preview(preview, &settings.look, settings.cursor);
        }
        let theme = &self.theme;
        self.section("Colour", page.colour + by);
        self.section("Style", page.style + by);
        let named = match self.hover {
            Some(Widget::Cursor(index)) => index,
            _ => settings.cursor,
        };
        self.section(&format!("Cursor \u{b7} {}", CURSOR_STYLES[named].name), page.cursor + by);
        self.section("Shortcuts", page.keys + by);
        self.text(TIP, &self.fonts.small, rect(MARGIN, page.tip + by, INNER, TIP_HEIGHT), theme.muted);
        self.text(&Self::version(), &self.fonts.small_line, rect(MARGIN, page.version + by, INNER, 20.0), theme.muted);
        let presets = settings::presets();
        let active = presets.iter().position(|preset| preset.look == settings.look);
        let mut picker = None;
        for (widget, area) in self.layout() {
            let hovered = self.hover == Some(widget);
            match widget {
                Widget::Chip(index) => self.chip(area, &presets[index], index, active == Some(index), hovered),
                Widget::NewPreset => {
                    let fill = if hovered { theme.control_hover } else { theme.control };
                    self.fill_round(area, CHIP_HEIGHT / 2.0, fill);
                    self.outline_round(area, CHIP_HEIGHT / 2.0, theme.control_border, 1.0);
                    self.icon(ICON_ADD, area, theme.text);
                }
                Widget::Swatch(index) => self.swatch(area, index, settings.look.color, hovered),
                Widget::Style(index) => {
                    let selected = settings.look.style == STYLES[index];
                    self.segment(area, STYLE_NAMES[index], selected, hovered, index);
                }
                Widget::Slider(index) => self.slider(index, area.top, settings, hovered),
                Widget::Toggle(index) => self.toggle(index, area, settings, hovered),
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
                Widget::Binding(slot) => {
                    let top = area.top - 7.0;
                    let (label, scope) = row_label(slot);
                    self.text(&label, &self.fonts.body, rect(MARGIN, top + 3.0, 176.0, 22.0), theme.text);
                    self.text(scope, &self.fonts.small_line, rect(MARGIN, top + 23.0, 176.0, 16.0), theme.muted);
                    let (field, clear) = Self::binding_box(top);
                    self.binding_field(slot, field, clear);
                }
                Widget::More => self.more_row(area, hovered),
                Widget::Link(index) => {
                    let label = &links()[index].0;
                    let color = if hovered { theme.text } else { theme.accent };
                    self.text(label, &self.fonts.small_line, area, color);
                    if hovered {
                        self.fill(rect(area.left, area.bottom - 3.0, area.right - area.left, 1.0), color);
                    }
                    self.text("\u{b7}", &self.fonts.small_line, rect(area.left - 10.0, area.top, 8.0, 20.0), theme.muted);
                }
                Widget::Reset => self.button(area, "Reset pen & options", hovered, theme.text),
                Widget::Quit => self.button(area, "Quit", hovered, theme.danger),
                Widget::Picker => picker = Some(area),
                _ => {}
            }
        }
        // Last, over the style and sliders it covers.
        if picker.is_some() {
            let swatches = Self::swatch_rects(page.colour + 22.0 + by);
            self.draw_picker(&Picker::under(swatches[0]), settings.look.color);
        }
    }

    /// The colour picker: a hue bar, a shade bar from dark to pale, and the colour they make.
    unsafe fn draw_picker(&self, picker: &Picker, color: u32) {
        let Some((hue, saturation, light)) = self.picking else { return };
        let theme = &self.theme;
        let card = picker.card;
        self.fill_round(rect(card.left, card.top + 2.0, INNER, card.bottom - card.top), 8.0, faded(hex(0x000000), 0.22));
        self.fill_round(card, 8.0, theme.control);
        self.outline_round(card, 8.0, theme.control_border, 1.0);
        self.text("Custom colour", &self.fonts.section, rect(card.left + 16.0, card.top + 12.0, 200.0, 20.0), theme.text);
        let (cx, cy) = (card.right - 28.0, card.top + 22.0);
        self.circle(cx, cy, 10.0, hex(color));
        self.ring(cx, cy, 10.0, faded(theme.text, 0.15), 1.0);
        self.text(&format!("#{color:06X}"), &self.fonts.body_right, rect(cx - 116.0, card.top + 12.0, 100.0, 20.0), theme.muted);
        let bar = |area: D2D_RECT_F, paint: &dyn Fn(f32) -> u32, at: f32| {
            let steps = 96;
            let width = (area.right - area.left) / steps as f32;
            self.target.PushAxisAlignedClip(&area, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            for step in 0..steps {
                let k = (step as f32 + 0.5) / steps as f32;
                self.fill(rect(area.left + step as f32 * width, area.top, width + 0.6, area.bottom - area.top), hex(paint(k)));
            }
            self.target.PopAxisAlignedClip();
            self.outline_round(area, 6.0, faded(theme.text, 0.12), 1.0);
            let x = area.left + (area.right - area.left) * at;
            let y = (area.top + area.bottom) / 2.0;
            self.circle(x, y, 9.0, hex(0xffffff));
            self.ring(x, y, 9.0, faded(hex(0x000000), 0.35), 1.0);
            self.circle(x, y, 6.0, hex(paint(at)));
        };
        bar(picker.hue, &|k| from_hsl((k, 1.0, 0.5)), hue);
        let shade = |k: f32| SHADES.0 + (SHADES.1 - SHADES.0) * k;
        let at = ((light - SHADES.0) / (SHADES.1 - SHADES.0)).clamp(0.0, 1.0);
        bar(picker.shade, &|k| from_hsl((hue, saturation, shade(k))), at);
        let hovered = self.hover == Some(Widget::PickerDone);
        self.button(picker.done, "Done", hovered, theme.text);
    }

    unsafe fn draw_preview(&mut self, area: D2D_RECT_F, look: &Look, cursor: usize) {
        let card = D2D1_ROUNDED_RECT {
            rect: area,
            radiusX: 8.0,
            radiusY: 8.0,
        };
        self.brush.SetColor(&self.theme.paper);
        self.target.FillRoundedRectangle(&card, &self.brush);
        // A pretend slide underneath.
        let sketch = self.theme.sketch;
        self.fill_round(rect(area.left + 16.0, area.top + 14.0, 140.0, 10.0), 3.0, sketch);
        for row in 0..3 {
            let y = area.top + 36.0 + row as f32 * 15.0;
            self.fill_round(rect(area.left + 16.0, y, 170.0 - row as f32 * 30.0, 6.0), 3.0, sketch);
        }
        for bar in 0..4 {
            let h = [24.0, 36.0, 30.0, 48.0][bar];
            let x = area.right - 100.0 + bar as f32 * 20.0;
            self.fill_round(rect(x, area.bottom - 14.0 - h, 12.0, h), 2.0, sketch);
        }
        self.target.PushAxisAlignedClip(&area, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        // The trail is kept from the preview's corner, and drawn moved to where it is now.
        let mut previous = Matrix3x2::default();
        self.target.GetTransform(&mut previous);
        self.target.SetTransform(&Matrix3x2 {
            M31: previous.M31 + area.left * previous.M11,
            M32: previous.M32 + area.top * previous.M22,
            ..previous
        });
        let mut preview = Stroke {
            points: std::mem::take(&mut self.preview.points),
            look: *look,
            scale: 1.0,
            whole: false,
            breaks: Vec::new(),
            flash: None,
        };
        let _ = stroke::draw(
            &self.factory,
            &self.target,
            &self.brush,
            &mut preview,
            Instant::now(),
            1.0,
            &mut self.preview.scratch,
        );
        self.target.SetTransform(&previous);
        self.preview.points = preview.points;
        let (hx, hy) = Preview::position(self.preview.t);
        let (sx, sy) = CURSOR_STYLES[cursor].hotspot;
        let zoom = 0.75;
        self.draw_cursor(cursor, look.color, area.left + hx - sx * zoom, area.top + hy - sy * zoom, zoom);
        self.target.PopAxisAlignedClip();
        self.brush.SetColor(&self.theme.border);
        self.target.DrawRoundedRectangle(&card, &self.brush, 1.0, None);
    }

    /// A preset: its colour, its name, and an outline when the pen is set as it says.
    unsafe fn chip(&self, area: D2D_RECT_F, preset: &Preset, index: usize, active: bool, hovered: bool) {
        let theme = &self.theme;
        let radius = CHIP_HEIGHT / 2.0;
        self.fill_round(area, radius, if hovered { theme.control_hover } else { theme.control });
        if active {
            self.outline_round(area, radius, theme.accent, 2.0);
        } else {
            self.outline_round(area, radius, theme.control_border, 1.0);
        }
        let color = hex(preset.look.color);
        let cy = (area.top + area.bottom) / 2.0;
        if preset.look.style != STYLE_SOLID {
            self.circle(area.left + 17.0, cy, 8.0, faded(color, 0.22));
        }
        self.circle(area.left + 17.0, cy, 5.0, color);
        let name_area = rect(area.left + 30.0, area.top, area.right - area.left - 42.0, CHIP_HEIGHT);
        match &self.renaming {
            Some(rename) if rename.index == index => {
                let field = rect(name_area.left - 4.0, area.top + 5.0, name_area.right - name_area.left + 8.0, CHIP_HEIGHT - 10.0);
                self.fill_round(field, 3.0, theme.background);
                self.outline_round(field, 3.0, theme.accent, 1.5);
                let shown = rename.text.as_str();
                let width = self.measure(shown, &self.fonts.body).min(name_area.right - name_area.left);
                if rename.fresh {
                    // Shown selected, since the first key replaces the whole name.
                    self.fill_round(rect(name_area.left, cy - 8.0, width, 16.0), 2.0, faded(theme.accent, 0.35));
                }
                self.text(shown, &self.fonts.body, name_area, theme.text);
                if !rename.fresh && self.started.elapsed().as_millis() % 1060 < 530 {
                    self.fill(rect(name_area.left + width + 1.0, cy - 8.0, 1.0, 16.0), theme.text);
                }
            }
            _ => self.text(&preset.name, &self.fonts.body, name_area, theme.text),
        }
    }

    /// The line that opens and closes More.
    unsafe fn more_row(&self, area: D2D_RECT_F, hovered: bool) {
        let theme = &self.theme;
        if hovered {
            self.fill_round(area, 6.0, theme.control_hover);
        }
        self.fill(rect(MARGIN, area.top, INNER, 1.0), theme.border);
        let left = area.left + 8.0;
        self.text("More", &self.fonts.section, rect(left, area.top + 4.0, 200.0, 22.0), theme.text);
        let detail = "Opacity, smoothing, ripples, start with Windows";
        self.text(detail, &self.fonts.small_line, rect(left, area.top + 24.0, 330.0, 16.0), theme.muted);
        let glyph = if self.more { ICON_UP } else { ICON_DOWN };
        self.icon(glyph, rect(area.right - 40.0, area.top, 32.0, area.bottom - area.top), theme.text);
    }

    unsafe fn draw_scroll_bar(&self) {
        let Some(thumb) = self.thumb() else { return };
        let held = matches!(self.dragging, Some(Drag::Thumb(_)));
        let wide = held || self.hover == Some(Widget::Thumb);
        let (width, alpha) = if wide { (6.0, 0.8) } else { (3.0, 0.5) };
        let bar = rect(WIDTH - 4.0 - width, thumb.top + 2.0, width, thumb.bottom - thumb.top - 4.0);
        self.fill_round(bar, width / 2.0, faded(self.theme.muted, alpha));
    }

    /// The strip along the bottom: what to do while recording a shortcut, or the latest message.
    unsafe fn draw_strip(&self) {
        let theme = &self.theme;
        let (text, color) = if self.capturing.is_some() && !self.status_showing() {
            ("Press the keys you want. Click anywhere else to cancel.".to_string(), theme.muted)
        } else if let Some((text, _, error)) = self.status.as_ref().filter(|_| self.status_showing()) {
            (text.clone(), if *error { theme.danger } else { theme.muted })
        } else {
            return;
        };
        let strip = rect(0.0, HEIGHT - STRIP, WIDTH, STRIP);
        self.fill(strip, theme.background);
        self.fill(rect(0.0, strip.top, WIDTH, 1.0), theme.border);
        self.text(&text, &self.fonts.small_line, rect(MARGIN, strip.top + 1.0, INNER, STRIP - 1.0), color);
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
            // An invitation rather than a greyed-out box: no fill, the accent's text.
            self.fill_round(field, 4.0, if hovered { theme.control_hover } else { theme.background });
            self.outline_round(field, 4.0, if hovered { theme.accent } else { theme.control_border }, 1.0);
            let label = "Add shortcut";
            let width = self.measure(label, &self.fonts.body) + 22.0;
            let left = (field.left + field.right - width) / 2.0;
            self.icon(ICON_ADD, rect(left, field.top, 16.0, field.bottom - field.top), theme.accent);
            self.text(label, &self.fonts.body, rect(left + 22.0, field.top, width, field.bottom - field.top), theme.accent);
        }
        if binding.is_set() {
            if self.hover == Some(Widget::Unbind(slot)) {
                self.fill_round(clear, 4.0, theme.control_hover);
            }
            self.icon(ICON_CLOSE, clear, theme.muted);
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

    unsafe fn slider(&self, index: usize, y: f32, settings: &Settings, hovered: bool) {
        let theme = &self.theme;
        let slider = &SLIDERS[index];
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
        let inner = if enabled && (hovered || self.dragging == Some(Drag::Slider(index))) { 7.0 } else { 5.0 };
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

    fn canvas(dpi: u32) -> snapshot::Canvas {
        snapshot::init();
        let scale = dpi as f32 / 96.0;
        snapshot::Canvas::new((WIDTH * scale) as u32, (HEIGHT * scale) as u32, dpi)
    }

    fn panel(canvas: &snapshot::Canvas, light: bool, dpi: u32) -> Panel {
        unsafe { Panel::with_target(canvas.factory.clone(), canvas.target.clone(), dpi, Theme::new(light, Some(0x0078d4))) }.unwrap()
    }

    fn overlaps(a: &D2D_RECT_F, b: &D2D_RECT_F) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    #[test]
    fn the_picker_reads_back_the_colours_it_makes() {
        for hue in [0.0, 0.13, 0.4, 0.58, 0.83] {
            for saturation in [0.4, 1.0] {
                for light in [0.2, 0.5, 0.8] {
                    let (h, s, l) = to_hsl(from_hsl((hue, saturation, light)));
                    let close = (h - hue).abs() < 0.01 && (s - saturation).abs() < 0.02 && (l - light).abs() < 0.01;
                    assert!(close, "{hue}, {saturation}, {light} came back as {h}, {s}, {l}");
                }
            }
        }
        // The swatches, softer colours, greys, black and white all come back as themselves, so
        // opening the picker never changes the colour.
        for rgb in [0xff2a2a, 0x2ecc40, 0x1e90ff, 0x808080, 0x000000, 0xffffff, 0x6b8e23] {
            let back = from_hsl(to_hsl(rgb));
            let close = |shift: u32| ((rgb >> shift) & 0xff).abs_diff((back >> shift) & 0xff) <= 1;
            assert!(close(16) && close(8) && close(0), "{rgb:06x} came back as {back:06x}");
        }
        assert_eq!(from_hsl((0.0, 1.0, 0.5)), 0xff0000);
        assert_eq!(from_hsl((1.0 / 3.0, 1.0, 0.5)), 0x00ff00);
        assert_eq!(from_hsl((2.0 / 3.0, 1.0, 0.5)), 0x0000ff);
    }

    /// Scrolled anywhere, with More open or shut, everything fits across the panel and nothing
    /// sits on anything else; and the page scrolls far enough to show Quit whole.
    #[test]
    fn the_page_scrolls_through_everything_without_overlaps() {
        let canvas = canvas(96);
        let mut panel = panel(&canvas, false, 96);
        for more in [false, true] {
            panel.more = more;
            let max = panel.max_scroll();
            assert!(max > 0.0);
            let mut seen = Vec::new();
            let mut scroll = 0.0;
            while scroll < max + STEP {
                panel.scroll = f32::min(scroll, max);
                let widgets = panel.layout();
                for (index, (widget, area)) in widgets.iter().enumerate() {
                    assert!(area.left >= 0.0 && area.right <= WIDTH, "{widget:?} runs off the side at {area:?}");
                    for (other, other_area) in &widgets[index + 1..] {
                        if [*widget, *other].iter().any(|w| matches!(w, Widget::Power | Widget::Thumb)) {
                            continue;
                        }
                        assert!(!overlaps(area, other_area), "{widget:?} and {other:?} overlap, more {more}");
                    }
                    seen.push(*widget);
                }
                scroll += STEP;
            }
            panel.scroll = max;
            let quit = panel.area_of(Widget::Quit).expect("Quit shows at the end");
            assert!(quit.bottom <= HEIGHT && quit.top >= VIEW_TOP);
            for command in COMMANDS {
                assert!(seen.contains(&Widget::Binding(Slot::Command(command))), "no line for {command:?}");
            }
            for index in 0..TOGGLES.len() {
                assert_eq!(seen.contains(&Widget::Toggle(index)), index == KEEP_STROKES || more, "toggle {index}, more {more}");
            }
            assert_eq!(seen.contains(&Widget::Slider(1)), more);
        }
    }

    /// Giving a preset a shortcut from its menu adds its line and brings it into view, clear of
    /// the strip that says to press the keys.
    #[test]
    fn a_preset_shortcut_from_the_menu_shows_its_line() {
        let canvas = canvas(96);
        let mut panel = panel(&canvas, true, 96);
        let line = Widget::Binding(Slot::Preset(1));
        assert!(!panel.layout().iter().any(|(widget, _)| *widget == line));
        panel.preset_choice(1, PresetChoice::Shortcut);
        assert_eq!(panel.capturing, Some(Slot::Preset(1)));
        let area = panel.area_of(line).expect("the preset's line is in view");
        assert!(area.top >= VIEW_TOP && area.bottom <= HEIGHT - STRIP);
    }

    /// Dragging the thumb to the bottom shows the end of the page; a right-click on a preset asks
    /// for its menu.
    #[test]
    fn the_thumb_drags_and_presets_have_menus() {
        let canvas = canvas(96);
        let mut panel = panel(&canvas, true, 96);
        let thumb = panel.thumb().unwrap();
        panel.mouse_down(thumb.left + 6.0, thumb.top + 4.0);
        panel.mouse_move(thumb.left + 6.0, HEIGHT + 200.0);
        panel.mouse_up();
        assert_eq!(panel.scroll, panel.max_scroll());
        panel.scroll = 0.0;
        let chip = panel.area_of(Widget::Chip(2)).unwrap();
        assert!(matches!(panel.right_click(chip.left + 4.0, chip.top + 4.0), Action::PresetMenu(2)));
    }

    /// Writes the page at the top, at the shortcuts, at the end with More open, with the picker,
    /// recording a shortcut and renaming a preset, light and dark, at 100 % and 150 %, to
    /// target/snapshots for review.
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn snapshot_panel() {
        for (theme, light) in [("light", true), ("dark", false)] {
            for dpi in [96, 144] {
                let canvas = canvas(dpi);
                let mut panel = panel(&canvas, light, dpi);
                for _ in 0..40 {
                    panel.preview.last -= Duration::from_millis(16);
                    panel.tick();
                }
                let shoot = |panel: &mut Panel, name: &str| {
                    unsafe { panel.draw() };
                    canvas.save(&format!("panel-{name}-{theme}-{dpi}.png"));
                };
                shoot(&mut panel, "top");
                panel.hover = Some(Widget::Cursor(3));
                panel.scroll = unsafe { panel.page() }.cursor - 8.0;
                shoot(&mut panel, "shortcuts");
                panel.hover = None;
                panel.more = true;
                panel.scroll = panel.max_scroll();
                shoot(&mut panel, "more");
                panel.scroll = 0.0;
                panel.picking = Some(to_hsl(0x1e90ff));
                shoot(&mut panel, "picker");
                panel.picking = None;
                panel.preset_choice(1, PresetChoice::Shortcut);
                shoot(&mut panel, "capture");
                panel.capturing = None;
                panel.scroll = 0.0;
                panel.preset_choice(2, PresetChoice::Rename);
                panel.char(u32::from('M'));
                shoot(&mut panel, "rename");
            }
        }
    }
}
