//! User settings, presets and shortcuts, kept in %APPDATA%\LaserPointer\settings.ini.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::OnceLock;

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
use windows::Win32::Storage::Packaging::Appx::{GetCurrentPackageFamilyName, GetCurrentPackageFullName};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, VK_ESCAPE, VK_OEM_6,
};

use crate::cursors;

pub const STYLE_SOLID: u32 = 0;
pub const STYLE_GLOW: u32 = 1;
pub const STYLE_NAMES: [&str; 2] = ["Solid", "Glow"];

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
        Some(Self::new(mods.trim().parse().ok()?, vk.trim().parse().ok()?))
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
    pub smoothing: u32,
    pub style: u32,
    /// Strokes stay until cleared instead of fading.
    pub ink: bool,
    /// How readily a finished stroke is straightened into a line, circle, box or arrow, 0–100.
    pub snap: u32,
}

impl Look {
    #[allow(clippy::too_many_arguments)]
    const fn new(
        color: u32,
        width: u32,
        opacity: u32,
        fade_ms: u32,
        style: u32,
        ink: bool,
        snap: u32,
    ) -> Self {
        Self {
            color,
            width,
            opacity,
            fade_ms,
            smoothing: 4,
            style,
            ink,
            snap,
        }
    }

    fn clamped(mut self) -> Self {
        self.color &= 0xffffff;
        self.width = self.width.clamp(2, 24);
        self.opacity = self.opacity.clamp(10, 100);
        self.fade_ms = self.fade_ms.clamp(200, 5000);
        self.smoothing = self.smoothing.min(8);
        // Neon, a third style older versions had, draws as Glow now.
        self.style = self.style.min(STYLE_GLOW);
        self.snap = self.snap.min(100);
        self
    }

    /// One line describing it, for a preset's tooltip.
    pub fn summary(&self) -> String {
        let mut text = format!("{} px \u{b7} {}", self.width, STYLE_NAMES[self.style as usize]);
        if self.opacity < 100 {
            text.push_str(&format!(" \u{b7} {}%", self.opacity));
        }
        if self.ink {
            text.push_str(" \u{b7} keeps strokes");
        } else {
            text.push_str(&format!(" \u{b7} fades in {:.1} s", self.fade_ms as f32 / 1000.0));
        }
        text
    }
}

const LASER: Look = Look::new(0xff2a2a, 4, 100, 1200, STYLE_GLOW, false, 70);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Toggle,
    Off,
    Undo,
    Redo,
    Clear,
    ToggleInk,
    NextPreset,
}

pub const COMMANDS: [Command; 7] = [
    Command::Toggle,
    Command::Off,
    Command::Undo,
    Command::Redo,
    Command::Clear,
    Command::ToggleInk,
    Command::NextPreset,
];

impl Command {
    pub fn index(self) -> usize {
        COMMANDS.iter().position(|&command| command == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Toggle => "Turn laser on or off",
            Self::Off => "Turn laser off",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Clear => "Clear strokes",
            Self::ToggleInk => "Keep strokes on or off",
            Self::NextPreset => "Next preset",
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
        }
    }

    fn default_binding(self) -> Binding {
        let (alt, ctrl) = (MOD_ALT.0, MOD_CONTROL.0);
        match self {
            Self::Toggle => Binding::new(alt, u32::from(b'L')),
            Self::Off => Binding::new(0, u32::from(VK_ESCAPE.0)),
            Self::Undo => Binding::new(ctrl, u32::from(b'Z')),
            Self::Redo => Binding::new(ctrl, u32::from(b'Y')),
            Self::Clear => Binding::new(alt, u32::from(b'X')),
            Self::ToggleInk => Binding::new(alt, u32::from(b'K')),
            Self::NextPreset => Binding::new(alt, u32::from(VK_OEM_6.0)),
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
    /// Index into `cursors::STYLES`.
    pub cursor: usize,
    pub ripples: bool,
    pub indicator: bool,
    /// Take the mouse from a game that has captured it while the laser is on.
    pub game_focus: bool,
    /// A ring closes in on the cursor as the laser turns on, so the audience can find it.
    pub pulse: bool,
    /// The first notice has taught how to draw and stop; it says only "Laser on" from then on.
    pub hinted: bool,
    pub keys: [Binding; COMMANDS.len()],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            look: LASER,
            cursor: 0,
            ripples: true,
            indicator: true,
            game_focus: true,
            pulse: false,
            hinted: false,
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
            Look::new(0xffd400, 18, 40, 3000, STYLE_SOLID, false, 70),
        ),
        preset(
            "Pen",
            Look::new(0x1e90ff, 4, 100, 1200, STYLE_SOLID, true, 70),
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
    settings.cursor = settings.cursor.min(cursors::STYLES.len() - 1);
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
    PRESETS.with(|presets| presets.borrow().iter().position(|preset| preset.look == look))
}

pub fn apply_preset(index: usize) -> Option<String> {
    let preset = PRESETS.with(|presets| presets.borrow().get(index).cloned())?;
    let mut settings = get();
    settings.look = preset.look;
    set(settings);
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
        Slot::Preset(index) => presets().get(index).map_or(Binding::NONE, |preset| preset.key),
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

// ---- The settings file ---------------------------------------------------------------------------

type Field = (&'static str, fn(&Settings) -> u32, fn(&mut Settings, u32));

const FIELDS: [Field; 13] = [
    ("color", |s| s.look.color, |s, v| s.look.color = v),
    ("width", |s| s.look.width, |s, v| s.look.width = v),
    ("opacity", |s| s.look.opacity, |s, v| s.look.opacity = v),
    ("fade_ms", |s| s.look.fade_ms, |s, v| s.look.fade_ms = v),
    ("smoothing", |s| s.look.smoothing, |s, v| s.look.smoothing = v),
    ("style", |s| s.look.style, |s, v| s.look.style = v),
    ("ink", |s| s.look.ink.into(), |s, v| s.look.ink = v != 0),
    ("snap", |s| s.look.snap, |s, v| s.look.snap = v),
    ("ripples", |s| s.ripples.into(), |s, v| s.ripples = v != 0),
    ("indicator", |s| s.indicator.into(), |s, v| s.indicator = v != 0),
    ("game_focus", |s| s.game_focus.into(), |s, v| s.game_focus = v != 0),
    ("pulse", |s| s.pulse.into(), |s, v| s.pulse = v != 0),
    ("hinted", |s| s.hinted.into(), |s, v| s.hinted = v != 0),
];

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("LaserPointer").join("settings.ini"))
}

fn number(value: &str) -> Option<u32> {
    match value.strip_prefix('#') {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

/// name|#rrggbb|width|opacity|fade|trail|smoothing|style|ink|mods|vk[|snap]
/// Snap came later, so it goes last and files without it still load. Trail length is gone (the
/// trail lasts as long as its fade), but its column stays so the others don't move.
fn parse_preset(value: &str) -> Option<Preset> {
    let parts: Vec<&str> = value.split('|').collect();
    let (fields, snap) = match parts.len() {
        11 => (&parts[..], None),
        12 => (&parts[..11], Some(number(parts[11])?)),
        _ => return None,
    };
    let [name, color, width, opacity, fade, _trail, smoothing, style, ink, mods, vk] = fields else {
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
            smoothing: number(smoothing)?,
            style: number(style)?,
            ink: number(ink)? != 0,
            snap: snap.unwrap_or(LASER.snap),
        },
        key: Binding::new(number(mods)?, number(vk)?),
    })
}

/// Reads a settings file. Presets are `None` when the file never listed any, so the defaults stay.
fn parse(text: &str) -> (Settings, Option<Vec<Preset>>) {
    let mut settings = Settings::default();
    let mut presets: Option<Vec<Preset>> = None;
    let mut old_hotkey = Binding::NONE;
    let mut toggle_read = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "preset" {
            presets.get_or_insert_with(Vec::new).extend(parse_preset(value));
        } else if key == "presets" {
            // Written before the list, so a list the user emptied loads as empty, not as the
            // defaults an older file without presets gets.
            presets.get_or_insert_with(Vec::new);
        } else if key == "cursor" {
            settings.cursor = cursors::index_of(value);
        } else if let Some(command) = COMMANDS.iter().find(|command| command.key() == key) {
            if let Some(binding) = Binding::parse(value) {
                settings.keys[command.index()] = binding;
                toggle_read |= *command == Command::Toggle;
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
    // The one shortcut older versions had, unless the file also says what Toggle is now.
    if old_hotkey.is_set() && !toggle_read {
        settings.keys[Command::Toggle.index()] = old_hotkey;
    }
    (settings, presets)
}

fn serialize(settings: &Settings, presets: &[Preset]) -> String {
    let mut text = String::new();
    for (name, get, _) in FIELDS {
        let value = get(settings);
        if name == "color" {
            text.push_str(&format!("{name}=#{value:06x}\n"));
        } else {
            text.push_str(&format!("{name}={value}\n"));
        }
    }
    text.push_str(&format!("cursor={}\n", cursors::STYLES[settings.cursor].id));
    for command in COMMANDS {
        let key = settings.key(command);
        text.push_str(&format!("{}={},{}\n", command.key(), key.mods, key.vk));
    }
    text.push_str(&format!("presets={}\n", presets.len()));
    for preset in presets {
        let look = preset.look;
        text.push_str(&format!(
            "preset={}|#{:06x}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            preset.name,
            look.color,
            look.width,
            look.opacity,
            look.fade_ms,
            0,
            look.smoothing,
            look.style,
            u32::from(look.ink),
            preset.key.mods,
            preset.key.vk,
            look.snap,
        ));
    }
    text
}

/// Returns true on first run, when there was no settings file yet.
pub fn load() -> bool {
    let text = path().and_then(|path| std::fs::read_to_string(path).ok());
    let first_run = text.is_none();
    let (settings, list) = parse(text.as_deref().unwrap_or_default());
    set(settings);
    if let Some(list) = list {
        set_presets(list);
    }
    first_run
}

/// Writes a temporary file and renames it over the old one, so a crash or full disk mid-write
/// can't leave a truncated settings file behind.
pub fn save() {
    let Some(path) = path() else { return };
    let text = serialize(&get(), &presets());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("ini.tmp");
    if std::fs::write(&temporary, text).is_ok() {
        let _ = std::fs::rename(&temporary, &path);
    }
}

// ---- Start with Windows --------------------------------------------------------------------------
// The plain exe adds itself to the Run key. The Store build is packaged, where that key would be
// virtualised away; it starts through the StartupTask its manifest declares (packaging/). The user
// can turn that off in Task Manager and an organisation can decide it by policy, and Windows won't
// let the app override either, so its state is always read back rather than assumed.

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUN_VALUE: PCWSTR = w!("LaserPointer");
/// Must match the TaskId in packaging/AppxManifest.xml.
const STARTUP_TASK: &str = "LaserPointerStartup";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autostart {
    Off,
    On,
    /// Turned off in Task Manager or Settings; only the user can turn it back on, there.
    OffByUser,
    /// Decided by the organisation's policy, on or off.
    Policy(bool),
    /// Not known yet: Windows is still answering.
    Unknown,
    /// Windows couldn't read or change it.
    Failed,
}

/// The Store package's family name, in the Store build; the plain exe has none.
pub fn package_family() -> Option<String> {
    if !packaged() {
        return None;
    }
    let mut name = [0u16; 128];
    let mut length = name.len() as u32;
    let found = unsafe { GetCurrentPackageFamilyName(&mut length, Some(PWSTR(name.as_mut_ptr()))) };
    (found.0 == 0 && length > 0).then(|| String::from_utf16_lossy(&name[..length as usize - 1]))
}

fn packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let mut length = 0;
        unsafe { GetCurrentPackageFullName(&mut length, None) != APPMODEL_ERROR_NO_PACKAGE }
    })
}

/// Asks where start-up stands, changing it first if `change` says so, and gives the answer to
/// `answer`. The Run key answers at once, on this thread. The StartupTask is asked on a worker
/// thread, so a slow answer from Windows never holds up the settings panel.
pub fn autostart(change: Option<bool>, answer: impl FnOnce(Autostart) + Send + 'static) {
    if packaged() {
        std::thread::spawn(move || answer(startup_task(change)));
    } else {
        if let Some(on) = change {
            set_run_key(on);
        }
        answer(run_key())
    }
}

fn startup_task(change: Option<bool>) -> Autostart {
    let state = (|| {
        let task = StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK))?.join()?;
        match change {
            Some(true) => return task.RequestEnableAsync()?.join(),
            Some(false) => task.Disable()?,
            None => {}
        }
        task.State()
    })();
    match state {
        Ok(StartupTaskState::Enabled) => Autostart::On,
        Ok(StartupTaskState::DisabledByUser) => Autostart::OffByUser,
        Ok(StartupTaskState::EnabledByPolicy) => Autostart::Policy(true),
        Ok(StartupTaskState::DisabledByPolicy) => Autostart::Policy(false),
        Ok(_) => Autostart::Off,
        Err(_) => Autostart::Failed,
    }
}

fn run_key() -> Autostart {
    let found = unsafe {
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
    };
    if found {
        Autostart::On
    } else {
        Autostart::Off
    }
}

fn set_run_key(on: bool) {
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
    fn round_trips_settings_and_presets() {
        let mut settings = Settings::default();
        settings.look.color = 0x123456;
        settings.look.snap = 80;
        settings.cursor = cursors::index_of("chevron");
        settings.game_focus = false;
        settings.pulse = true;
        settings.hinted = true;
        settings.keys[Command::Redo.index()] = Binding::new(MOD_CONTROL.0, u32::from(b'R'));
        let mut presets = default_presets();
        presets[1].key = Binding::new(MOD_ALT.0, u32::from(b'H'));

        let (parsed, parsed_presets) = parse(&serialize(&settings, &presets));
        assert_eq!(parsed, settings);
        assert_eq!(parsed_presets, Some(presets));
    }

    #[test]
    fn deleting_every_preset_sticks() {
        let (_, presets) = parse(&serialize(&Settings::default(), &[]));
        assert_eq!(presets, Some(Vec::new()));
    }

    #[test]
    fn empty_file_gives_defaults_and_keeps_default_presets() {
        let (settings, presets) = parse("");
        assert_eq!(settings, Settings::default());
        assert_eq!(presets, None);
    }

    #[test]
    fn loads_files_from_before_snap_redo_cursors_and_games() {
        let old = "color=#ff2a2a\nwidth=6\nink=0\ndot_cursor=0\nkey_toggle=1,76\n\
                   preset=Laser|#ff2a2a|4|100|1000|500|2|1|0|0|0\n";
        let (settings, presets) = parse(old);
        assert_eq!(settings.look.width, 6);
        // The old dot-or-pen switch has nothing to map to; the default cursor takes over.
        assert_eq!(settings.cursor, 0);
        assert!(settings.game_focus);
        assert_eq!(settings.key(Command::Redo), Command::Redo.default_binding());
        let presets = presets.unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].look.snap, LASER.snap);
    }

    /// A file with both the old shortcut and a Toggle line, however spaced, keeps the Toggle line,
    /// even when that line says Toggle has no shortcut.
    #[test]
    fn a_toggle_line_wins_over_the_old_single_hotkey() {
        let (settings, _) = parse("hotkey_mods=2
hotkey_vk=80
key_toggle = 0,0
");
        assert!(!settings.key(Command::Toggle).is_set());
    }

    #[test]
    fn the_old_single_hotkey_still_becomes_the_toggle() {
        let (settings, _) = parse("hotkey_mods=2\nhotkey_vk=80\n");
        assert_eq!(settings.key(Command::Toggle), Binding::new(2, 80));
    }

    #[test]
    fn a_saved_cursor_loads_and_an_unknown_one_falls_back() {
        let (settings, _) = parse("cursor=brackets\n");
        assert_eq!(settings.cursor, cursors::index_of("brackets"));
        let (settings, _) = parse("cursor=no-such-style\n");
        assert_eq!(settings.cursor, 0);
    }

    #[test]
    fn rejects_malformed_presets_and_bad_names() {
        assert_eq!(parse_preset("Only|#ff0000|4"), None);
        assert_eq!(parse_preset(" |#ff0000|4|100|1000|500|2|1|0|0|0"), None);
        assert_eq!(parse_preset("Pen|red|4|100|1000|500|2|1|0|0|0"), None);
    }

    /// Neon draws as Glow now, and the Previous preset and Quit shortcuts older versions saved
    /// are dropped without moving any other shortcut.
    #[test]
    fn neon_and_the_retired_shortcuts_load_without_harm() {
        let old = "style=2\nkey_previous_preset=1,219\nkey_quit=5,76\nkey_next_preset=1,221\n\
                   preset=Neon|#00e5ff|4|100|1000|0|2|2|0|0|0|70\n";
        let (mut settings, list) = parse(old);
        settings.look = settings.look.clamped();
        assert_eq!(settings.look.style, STYLE_GLOW);
        let mut expected = Settings::default().keys;
        expected[Command::NextPreset.index()] = Binding::new(1, 221);
        assert_eq!(settings.keys, expected);
        set_presets(list.unwrap());
        assert_eq!(presets()[0].look.style, STYLE_GLOW);
        assert!(!serialize(&settings, &[]).contains("key_quit"));
    }

    #[test]
    fn clean_name_strips_separators_and_control_characters() {
        assert_eq!(clean_name("  a|b\tc  "), "abc");
        assert_eq!(clean_name(&"x".repeat(40)).len(), MAX_NAME);
    }
}
