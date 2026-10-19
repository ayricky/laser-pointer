//! User settings, presets and shortcuts, kept in %APPDATA%\LaserPointer\settings.ini.

use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::PathBuf;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_ESCAPE, VK_OEM_4, VK_OEM_6,
};

pub const STYLE_SOLID: u32 = 0;
pub const STYLE_GLOW: u32 = 1;
pub const STYLE_NEON: u32 = 2;
pub const STYLE_NAMES: [&str; 3] = ["Solid", "Glow", "Neon"];

pub const MAX_PRESETS: usize = 8;
pub const MAX_NAME: usize = 24;

/// A key plus modifiers (MOD_* flags). A zero key means unbound.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Binding {
    pub mods: u32,
    pub vk: u32,
}

impl Binding {
    pub const NONE: Self = Self { mods: 0, vk: 0 };

    const fn new(mods: u32, vk: u32) -> Self {
        Self { mods, vk }
    }

    pub fn is_set(self) -> bool {
        self.vk != 0
    }

    fn parse(text: &str) -> Option<Self> {
        let (mods, vk) = text.split_once(',')?;
        Some(Self::new(
            mods.trim().parse().ok()?,
            vk.trim().parse().ok()?,
        ))
    }
}

/// Everything about how a stroke looks; what a preset captures.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    /// 0xRRGGBB
    pub color: u32,
    /// Stroke width at the head, in DIPs.
    pub width: u32,
    /// Percent.
    pub opacity: u32,
    pub fade_ms: u32,
    /// Distance behind the head over which the stroke tapers away, in DIPs.
    pub trail: u32,
    pub smoothing: u32,
    pub style: u32,
    /// Strokes stay until cleared instead of fading.
    pub ink: bool,
}

impl Look {
    const fn new(
        color: u32,
        width: u32,
        opacity: u32,
        fade_ms: u32,
        trail: u32,
        style: u32,
        ink: bool,
    ) -> Self {
        Self {
            color,
            width,
            opacity,
            fade_ms,
            trail,
            smoothing: 2,
            style,
            ink,
        }
    }

    fn clamped(mut self) -> Self {
        self.color &= 0xffffff;
        self.width = self.width.clamp(2, 24);
        self.opacity = self.opacity.clamp(10, 100);
        self.fade_ms = self.fade_ms.clamp(200, 5000);
        self.trail = self.trail.clamp(100, 2000);
        self.smoothing = self.smoothing.min(8);
        self.style = self.style.min(STYLE_NEON);
        self
    }

    /// One line describing it, for the presets list.
    pub fn summary(&self) -> String {
        let mut text = format!(
            "{} px \u{b7} {}",
            self.width, STYLE_NAMES[self.style as usize]
        );
        if self.opacity < 100 {
            text.push_str(&format!(" \u{b7} {}%", self.opacity));
        }
        if self.ink {
            text.push_str(" \u{b7} keeps strokes");
        } else {
            text.push_str(&format!(
                " \u{b7} fades in {:.1} s",
                self.fade_ms as f32 / 1000.0
            ));
        }
        text
    }
}

const LASER: Look = Look::new(0xff2a2a, 4, 100, 1000, 500, STYLE_GLOW, false);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Toggle,
    Off,
    Undo,
    Redo,
    Clear,
    ToggleInk,
    NextPreset,
    PreviousPreset,
    Quit,
}

pub const COMMANDS: [Command; 9] = [
    Command::Toggle,
    Command::Off,
    Command::Undo,
    Command::Redo,
    Command::Clear,
    Command::ToggleInk,
    Command::NextPreset,
    Command::PreviousPreset,
    Command::Quit,
];

impl Command {
    pub fn index(self) -> usize {
        COMMANDS
            .iter()
            .position(|&command| command == self)
            .unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Toggle => "Turn laser on or off",
            Self::Off => "Turn laser off",
            Self::Undo => "Undo last stroke",
            Self::Redo => "Redo",
            Self::Clear => "Clear strokes",
            Self::ToggleInk => "Keep strokes on or off",
            Self::NextPreset => "Next preset",
            Self::PreviousPreset => "Previous preset",
            Self::Quit => "Quit",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Toggle => "key_toggle",
            Self::Off => "key_off",
            Self::Undo => "key_undo",
            Self::Redo => "key_redo",
            Self::Clear => "key_clear",
            Self::ToggleInk => "key_ink",
            Self::NextPreset => "key_next_preset",
            Self::PreviousPreset => "key_previous_preset",
            Self::Quit => "key_quit",
        }
    }

    fn default_binding(self) -> Binding {
        let (alt, ctrl, shift) = (MOD_ALT.0, MOD_CONTROL.0, MOD_SHIFT.0);
        match self {
            Self::Toggle => Binding::new(alt, u32::from(b'L')),
            Self::Off => Binding::new(0, u32::from(VK_ESCAPE.0)),
            Self::Undo => Binding::new(ctrl, u32::from(b'Z')),
            Self::Redo => Binding::new(ctrl, u32::from(b'Y')),
            Self::Clear => Binding::new(alt, u32::from(b'X')),
            Self::ToggleInk => Binding::new(alt, u32::from(b'K')),
            Self::NextPreset => Binding::new(alt, u32::from(VK_OEM_6.0)),
            Self::PreviousPreset => Binding::new(alt, u32::from(VK_OEM_4.0)),
            Self::Quit => Binding::new(alt | shift, u32::from(b'L')),
        }
    }

    /// Bare keys like Esc are only claimed while they can mean something, so they never
    /// steal keys from other apps the rest of the time.
    pub fn scope(self) -> &'static str {
        match self {
            Self::Off | Self::Undo | Self::Redo => "while the laser is on",
            Self::Clear => "while strokes are on screen",
            _ => "always",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub look: Look,
    /// A glowing dot instead of the pen cursor.
    pub dot_cursor: bool,
    pub ripples: bool,
    pub indicator: bool,
    pub keys: [Binding; COMMANDS.len()],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            look: LASER,
            dot_cursor: true,
            ripples: true,
            indicator: true,
            keys: COMMANDS.map(Command::default_binding),
        }
    }
}

impl Settings {
    pub fn key(&self, command: Command) -> Binding {
        self.keys[command.index()]
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Preset {
    pub name: String,
    pub look: Look,
    pub key: Binding,
}

fn default_presets() -> Vec<Preset> {
    let preset = |name: &str, look: Look| Preset {
        name: name.into(),
        look,
        key: Binding::NONE,
    };
    vec![
        preset("Laser", LASER),
        preset(
            "Highlighter",
            Look::new(0xffd400, 18, 40, 3000, 2000, STYLE_SOLID, false),
        ),
        preset(
            "Pen",
            Look::new(0x1e90ff, 4, 100, 1000, 500, STYLE_SOLID, true),
        ),
    ]
}

/// Names live on one line of the settings file, split on '|'.
pub fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '|' && !c.is_control())
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

thread_local! {
    static CURRENT: Cell<Settings> = Cell::new(Settings::default());
    static PRESETS: RefCell<Vec<Preset>> = RefCell::new(default_presets());
}

pub fn get() -> Settings {
    CURRENT.with(Cell::get)
}

pub fn set(mut settings: Settings) {
    settings.look = settings.look.clamped();
    CURRENT.with(|current| current.set(settings));
}

pub fn presets() -> Vec<Preset> {
    PRESETS.with(|presets| presets.borrow().clone())
}

pub fn set_presets(mut list: Vec<Preset>) {
    list.truncate(MAX_PRESETS);
    for preset in &mut list {
        preset.look = preset.look.clamped();
    }
    PRESETS.with(|presets| *presets.borrow_mut() = list);
}

/// The preset the current look came from, if it still matches one.
pub fn active_preset() -> Option<usize> {
    let look = get().look;
    PRESETS.with(|presets| {
        presets
            .borrow()
            .iter()
            .position(|preset| preset.look == look)
    })
}

pub fn apply_preset(index: usize) -> Option<String> {
    let preset = PRESETS.with(|presets| presets.borrow().get(index).cloned())?;
    let mut settings = get();
    settings.look = preset.look;
    set(settings);
    save();
    Some(preset.name)
}

/// What uses a binding already, if anything; `except` is the slot being edited.
pub fn binding_owner(binding: Binding, except: Slot) -> Option<String> {
    let settings = get();
    for command in COMMANDS {
        if Slot::Command(command) != except && settings.key(command) == binding {
            return Some(command.label().into());
        }
    }
    presets()
        .iter()
        .enumerate()
        .find(|(index, preset)| Slot::Preset(*index) != except && preset.key == binding)
        .map(|(_, preset)| format!("the \u{201c}{}\u{201d} preset", preset.name))
}

/// Anything a shortcut can be assigned to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Command(Command),
    Preset(usize),
}

pub fn binding(slot: Slot) -> Binding {
    match slot {
        Slot::Command(command) => get().key(command),
        Slot::Preset(index) => presets()
            .get(index)
            .map_or(Binding::NONE, |preset| preset.key),
    }
}

pub fn set_binding(slot: Slot, binding: Binding) {
    match slot {
        Slot::Command(command) => {
            let mut settings = get();
            settings.keys[command.index()] = binding;
            set(settings);
        }
        Slot::Preset(index) => {
            let mut list = presets();
            if let Some(preset) = list.get_mut(index) {
                preset.key = binding;
            }
            set_presets(list);
        }
    }
    save();
}

type Field = (&'static str, fn(&Settings) -> u32, fn(&mut Settings, u32));

const FIELDS: [Field; 11] = [
    ("color", |s| s.look.color, |s, v| s.look.color = v),
    ("width", |s| s.look.width, |s, v| s.look.width = v),
    ("opacity", |s| s.look.opacity, |s, v| s.look.opacity = v),
    ("fade_ms", |s| s.look.fade_ms, |s, v| s.look.fade_ms = v),
    ("trail", |s| s.look.trail, |s, v| s.look.trail = v),
    (
        "smoothing",
        |s| s.look.smoothing,
        |s, v| s.look.smoothing = v,
    ),
    ("style", |s| s.look.style, |s, v| s.look.style = v),
    ("ink", |s| s.look.ink.into(), |s, v| s.look.ink = v != 0),
    (
        "dot_cursor",
        |s| s.dot_cursor.into(),
        |s, v| s.dot_cursor = v != 0,
    ),
    ("ripples", |s| s.ripples.into(), |s, v| s.ripples = v != 0),
    (
        "indicator",
        |s| s.indicator.into(),
        |s, v| s.indicator = v != 0,
    ),
];

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(base)
            .join("LaserPointer")
            .join("settings.ini"),
    )
}

fn number(value: &str) -> Option<u32> {
    match value.strip_prefix('#') {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

/// name|#rrggbb|width|opacity|fade|trail|smoothing|style|ink|mods|vk
fn parse_preset(value: &str) -> Option<Preset> {
    let parts: Vec<&str> = value.split('|').collect();
    let [name, color, width, opacity, fade, trail, smoothing, style, ink, mods, vk] = parts[..]
    else {
        return None;
    };
    let name = clean_name(name);
    if name.is_empty() {
        return None;
    }
    Some(Preset {
        name,
        look: Look {
            color: number(color)?,
            width: number(width)?,
            opacity: number(opacity)?,
            fade_ms: number(fade)?,
            trail: number(trail)?,
            smoothing: number(smoothing)?,
            style: number(style)?,
            ink: number(ink)? != 0,
        },
        key: Binding::new(number(mods)?, number(vk)?),
    })
}

pub fn parse(text: &str) -> (Settings, Option<Vec<Preset>>) {
    let mut settings = Settings::default();
    let mut list = Vec::new();
    let mut saw_presets = false;
    {
        let mut old_hotkey = Binding::NONE;
        let mut saw_toggle = false;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if key == "preset" {
                saw_presets = true;
                list.extend(parse_preset(value));
            } else if let Some(command) = COMMANDS.iter().find(|command| command.key() == key) {
                if let Some(binding) = Binding::parse(value) {
                    settings.keys[command.index()] = binding;
                    saw_toggle |= *command == Command::Toggle;
                }
            } else if let Some(value) = number(value) {
                match key {
                    // Older versions only had the one shortcut.
                    "hotkey_mods" => old_hotkey.mods = value,
                    "hotkey_vk" => old_hotkey.vk = value,
                    _ => {
                        if let Some((_, _, set)) = FIELDS.iter().find(|(name, ..)| *name == key) {
                            set(&mut settings, value);
                        }
                    }
                }
            }
        }
        if old_hotkey.is_set() && !saw_toggle {
            settings.keys[Command::Toggle.index()] = old_hotkey;
        }
    }
    settings.look = settings.look.clamped();
    list.truncate(MAX_PRESETS);
    for preset in &mut list {
        preset.look = preset.look.clamped();
    }
    (settings, saw_presets.then_some(list))
}

pub fn load() {
    let text = path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let (settings, list) = parse(&text);
    set(settings);
    if let Some(list) = list {
        set_presets(list);
    }
}

pub fn serialize(settings: &Settings, presets: &[Preset]) -> String {
    let mut text = String::new();
    for (name, get, _) in FIELDS {
        let value = get(settings);
        if name == "color" {
            text.push_str(&format!("{name}=#{value:06x}\n"));
        } else {
            text.push_str(&format!("{name}={value}\n"));
        }
    }
    for command in COMMANDS {
        let key = settings.key(command);
        text.push_str(&format!("{}={},{}\n", command.key(), key.mods, key.vk));
    }
    if presets.is_empty() {
        text.push_str("preset=\n");
    }
    for preset in presets {
        let look = preset.look;
        text.push_str(&format!(
            "preset={}|#{:06x}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            preset.name,
            look.color,
            look.width,
            look.opacity,
            look.fade_ms,
            look.trail,
            look.smoothing,
            look.style,
            u32::from(look.ink),
            preset.key.mods,
            preset.key.vk
        ));
    }
    text
}

/// Flush the replacement before publishing it, so a failed save leaves the last file intact.
fn save_file() -> std::io::Result<()> {
    let Some(path) = path() else { return Ok(()) };
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let temporary = path.with_extension("ini.tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(serialize(&get(), &presets()).as_bytes())?;
    file.sync_all()?;
    drop(file);
    unsafe {
        MoveFileExW(
            &HSTRING::from(temporary.as_os_str()),
            &HSTRING::from(path.as_os_str()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}

pub fn save() {
    if let Err(error) = save_file() {
        eprintln!("Could not save settings: {error}");
    }
}

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUN_VALUE: PCWSTR = w!("LaserPointer");

pub fn autostart() -> bool {
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
        .is_ok()
    }
}

pub fn set_autostart(on: bool) {
    unsafe {
        if !on {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let command = HSTRING::from(format!("\"{}\"", exe.display()));
        // REG_SZ data includes the terminating null.
        let bytes = (command.len() + 1) * 2;
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            bytes as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut settings = Settings {
            dot_cursor: false,
            ripples: false,
            indicator: false,
            ..Settings::default()
        };
        settings.look.ink = true;
        settings.look.color = 0x123456;
        settings.keys[Command::Redo.index()] = Binding::new(MOD_ALT.0, 0x71);
        let mut presets = default_presets();
        presets[0].name = "日本語 · café 🖊".into();
        presets[0].key = Binding::new(MOD_CONTROL.0, 0x72);
        let (parsed, list) = parse(&serialize(&settings, &presets));
        assert_eq!(parsed, settings);
        assert_eq!(list, Some(presets));
        assert_eq!(parse(&serialize(&settings, &[])).1, Some(Vec::new()));
    }

    #[test]
    fn old_shortcuts_and_missing_redo() {
        let (settings, presets) = parse("hotkey_mods=3\nhotkey_vk=75\n");
        assert_eq!(settings.key(Command::Toggle), Binding::new(3, 75));
        assert_eq!(
            settings.key(Command::Redo),
            Binding::new(MOD_CONTROL.0, u32::from(b'Y'))
        );
        assert!(presets.is_none());
        let (settings, _) = parse("hotkey_mods=3\nhotkey_vk=75\nkey_toggle = 0,0\n");
        assert_eq!(settings.key(Command::Toggle), Binding::NONE);
    }

    #[test]
    fn settings_and_presets_are_clamped() {
        let (settings, presets) = parse("color=#ffffffff\nwidth=0\nopacity=999\nfade_ms=1\ntrail=99999\nsmoothing=99\nstyle=999\npreset=Pen|#ffffffff|999|0|99999|0|99|99|1|0|0\n");
        assert_eq!(settings.look.color, 0xffffff);
        assert_eq!(settings.look.width, 2);
        assert_eq!(settings.look.opacity, 100);
        assert_eq!(settings.look.fade_ms, 200);
        assert_eq!(settings.look.trail, 2000);
        assert_eq!(settings.look.smoothing, 8);
        assert_eq!(settings.look.style, STYLE_NEON);
        let look = presets.unwrap()[0].look;
        assert_eq!(
            (look.width, look.opacity, look.fade_ms, look.trail),
            (24, 10, 5000, 100)
        );
        assert_eq!(
            (look.color, look.smoothing, look.style),
            (0xffffff, 8, STYLE_NEON)
        );
    }
}
