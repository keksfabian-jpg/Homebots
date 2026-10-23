//! Key bindings (GCInput): the binding sets BindingsFoot (Infantry), BindingsPCGlobal (Global),
//! BindingsLand and BindingsHeli of GCGame/Config/DefaultInput_<lang>.ini (else
//! DefaultInput.ini), command by EKBC_ CommandIndex; the user's changes from the options menu
//! (PC_U_KeyBindings) in homebots_settings.ini [Bindings].
//!
//! Key names are UE3's (EKeys): letters and digit words ("one") follow the keyboard layout like
//! UE3's virtual-key codes (Bevy logical keys), the others are physical keys; mouse buttons and
//! the wheel ("MouseScrollUp/Down") as in the ini.
//!
//! Game code asks `down(Set::Foot, "EKBC_MoveForward")` / `hit(..)` (pressed this frame); the
//! state is taken once per frame in PreUpdate.
use bevy::input::keyboard::Key;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Set {
    Foot,
    Global,
    Land,
    Heli,
}

impl Set {
    fn ini(self) -> &'static str {
        match self {
            Set::Foot => "BindingsFoot",
            Set::Global => "BindingsPCGlobal",
            Set::Land => "BindingsLand",
            Set::Heli => "BindingsHeli",
        }
    }
    /// InputActionToKey markup prefix (PC_U_KeyBindings: Infantry., Global., Land., Heli.)
    pub fn from_prefix(p: &str) -> Set {
        match p {
            "Global" => Set::Global,
            "Land" => Set::Land,
            "Heli" | "Air" => Set::Heli,
            _ => Set::Foot,
        }
    }
    const ALL: [Set; 4] = [Set::Foot, Set::Global, Set::Land, Set::Heli];
}

struct Binds {
    /// (set, EKBC command) -> key names in ini order (keyboard / mouse only)
    keys: BTreeMap<(Set, String), Vec<String>>,
    defaults: BTreeMap<(Set, String), Vec<String>>,
    /// CommandDescriptions: Description -> CommandIndex
    desc: BTreeMap<String, String>,
}

#[derive(Default)]
struct Snapshot {
    down: HashSet<(Set, String)>,
    hit: HashSet<(Set, String)>,
}

static BINDS: RwLock<Option<Binds>> = RwLock::new(None);
static SNAP: RwLock<Option<Snapshot>> = RwLock::new(None);

fn is_pad(name: &str) -> bool {
    name.starts_with("XboxTypeS_") || name.starts_with("Gamepad_") || name.starts_with("Joystick_") || name == "MouseX" || name == "MouseY"
}

/// Reads the binding sets of the game's DefaultInput ini (language variant first).
pub fn init(game: &std::path::Path, lang: &str) {
    let dir = game.join("GCGame").join("Config");
    let text = std::fs::read_to_string(dir.join(format!("DefaultInput_{lang}.ini"))).or_else(|_| std::fs::read_to_string(dir.join("DefaultInput.ini"))).unwrap_or_default();
    let mut defaults: BTreeMap<(Set, String), Vec<String>> = BTreeMap::new();
    let mut desc = BTreeMap::new();
    let field = |rest: &str, k: &str| -> Option<String> {
        let i = rest.find(k)?;
        let v = &rest[i + k.len()..];
        Some(v.trim_start_matches('"').split(['"', ',', ')']).next().unwrap_or("").to_string())
    };
    for l in text.lines() {
        let l = l.trim().trim_start_matches(['.', '+']);
        if l.starts_with(';') {
            continue;
        }
        if let Some(rest) = l.strip_prefix("CommandDescriptions=(") {
            if let (Some(ci), Some(d)) = (field(rest, "CommandIndex="), field(rest, "Description=")) {
                desc.entry(d).or_insert(ci);
            }
            continue;
        }
        for set in Set::ALL {
            let Some(rest) = l.strip_prefix(&format!("{}=(", set.ini())) else { continue };
            // modifier combinations (Control=True: debug bot commands) are not bindable here
            if rest.contains("Control=True") || rest.contains("Shift=True") || rest.contains("Alt=True") {
                continue;
            }
            let (Some(name), Some(ci)) = (field(rest, "Name="), field(rest, "CommandIndex=")) else { continue };
            if is_pad(&name) || ci == "EKBC_NOP" {
                continue;
            }
            let v = defaults.entry((set, ci)).or_default();
            if !v.iter().any(|k| k.eq_ignore_ascii_case(&name)) {
                v.push(name);
            }
        }
    }
    // the ini lists the bindings bottom-up in places (BindingsFoot "V" before
    // "MiddleMouseButton"): keyboard keys first, then mouse buttons
    for v in defaults.values_mut() {
        v.sort_by_key(|k| k.contains("Mouse"));
    }
    let mut keys = defaults.clone();
    // the user's [Bindings]: "Foot.EKBC_Reload=R,MiddleMouseButton"
    if let Ok(t) = std::fs::read_to_string(crate::settings::FILE) {
        let mut sec = String::new();
        for l in t.lines() {
            let l = l.trim();
            if l.starts_with('[') {
                sec = l.trim_matches(['[', ']']).to_string();
                continue;
            }
            if sec != "Bindings" {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let Some((s, ci)) = k.split_once('.') else { continue };
            let set = match s {
                "Foot" => Set::Foot,
                "Global" => Set::Global,
                "Land" => Set::Land,
                "Heli" => Set::Heli,
                _ => continue,
            };
            keys.insert((set, ci.to_string()), v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
        }
    }
    info!("input: {} bindings, {} command descriptions", defaults.len(), desc.len());
    *BINDS.write().unwrap() = Some(Binds { keys, defaults, desc });
}

/// [Bindings] lines of homebots_settings.ini (only the ones that differ from the ini).
pub fn save_lines() -> String {
    let g = BINDS.read().unwrap();
    let Some(b) = g.as_ref() else { return String::new() };
    let mut out = String::new();
    for (k, v) in &b.keys {
        if b.defaults.get(k) != Some(v) {
            out.push_str(&format!("{:?}.{}={}\n", k.0, k.1, v.join(",")));
        }
    }
    out
}

/// InputActionToKey markup ("Infantry.Reload", "Global.Seat0", "NextWeapon") -> binding.
/// The name is the command's Description (CommandDescriptions), else EKBC_<name>.
pub fn resolve(markup: &str) -> Option<(Set, String)> {
    let (set, name) = match markup.split_once('.') {
        Some((p, n)) => (Set::from_prefix(p), n),
        None => (Set::Foot, markup),
    };
    let g = BINDS.read().unwrap();
    let b = g.as_ref()?;
    let ci = b.desc.get(name).cloned().or_else(|| {
        let c = format!("EKBC_{name}");
        b.defaults.keys().any(|k| k.1 == c).then_some(c)
    })?;
    Some((set, ci))
}

/// Bound key names of a command (primary, secondary).
pub fn keys_of(set: Set, cmd: &str) -> Vec<String> {
    BINDS.read().unwrap().as_ref().and_then(|b| b.keys.get(&(set, cmd.to_string())).cloned()).unwrap_or_default()
}

/// Binds `key` to slot 0 (primary) / 1 (secondary); the key leaves other commands of the set.
pub fn bind(set: Set, cmd: &str, slot: usize, key: &str) {
    let _touch = Touch;
    let mut g = BINDS.write().unwrap();
    let Some(b) = g.as_mut() else { return };
    for ((s, c), v) in b.keys.iter_mut() {
        if *s == set && c != cmd {
            v.retain(|k| !k.eq_ignore_ascii_case(key));
        }
    }
    let v = b.keys.entry((set, cmd.to_string())).or_default();
    v.retain(|k| !k.eq_ignore_ascii_case(key));
    if slot < v.len() {
        v[slot] = key.to_string();
    } else {
        v.push(key.to_string());
    }
}

/// Removes the key in `slot` of a command.
pub fn unbind(set: Set, cmd: &str, slot: usize) {
    let _touch = Touch;
    let mut g = BINDS.write().unwrap();
    if let Some(v) = g.as_mut().and_then(|b| b.keys.get_mut(&(set, cmd.to_string()))) {
        if slot < v.len() {
            v.remove(slot);
        }
    }
}

pub fn reset() {
    let _touch = Touch;
    let mut g = BINDS.write().unwrap();
    if let Some(b) = g.as_mut() {
        b.keys = b.defaults.clone();
    }
}

/// Held this frame.
pub fn down(set: Set, cmd: &str) -> bool {
    SNAP.read().unwrap().as_ref().is_some_and(|s| s.down.contains(&(set, cmd.to_string())))
}

/// Pressed this frame.
pub fn hit(set: Set, cmd: &str) -> bool {
    SNAP.read().unwrap().as_ref().is_some_and(|s| s.hit.contains(&(set, cmd.to_string())))
}

/// UE3 key name -> physical key (named keys; letters / digits go by logical key).
fn named(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match name.to_ascii_lowercase().as_str() {
        "leftshift" => ShiftLeft,
        "rightshift" => ShiftRight,
        "leftcontrol" => ControlLeft,
        "rightcontrol" => ControlRight,
        "leftalt" => AltLeft,
        "rightalt" => AltRight,
        "spacebar" => Space,
        "tab" => Tab,
        "enter" => Enter,
        "escape" => Escape,
        "backspace" => Backspace,
        "capslockkey" | "capslock" => CapsLock,
        "tilde" => Backquote,
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "insert" => Insert,
        "delete" => Delete,
        "home" => Home,
        "end" => End,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        "numpadzero" => Numpad0,
        "numpadone" => Numpad1,
        "numpadtwo" => Numpad2,
        "numpadthree" => Numpad3,
        "numpadfour" => Numpad4,
        "numpadfive" => Numpad5,
        "numpadsix" => Numpad6,
        "numpadseven" => Numpad7,
        "numpadeight" => Numpad8,
        "numpadnine" => Numpad9,
        "multiply" => NumpadMultiply,
        "add" => NumpadAdd,
        "subtract" => NumpadSubtract,
        "decimal" => NumpadDecimal,
        "divide" => NumpadDivide,
        "semicolon" | "colon" => Semicolon,
        "equals" => Equal,
        "comma" => Comma,
        "underscore" => Minus,
        "period" => Period,
        "slash" => Slash,
        "leftbracket" => BracketLeft,
        "rightbracket" => BracketRight,
        "backslash" => Backslash,
        "quote" => Quote,
        _ => return None,
    })
}

const DIGITS: [&str; 10] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];

/// The character a letter / digit-word key name stands for.
fn char_of(name: &str) -> Option<String> {
    if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(name.to_ascii_lowercase());
    }
    DIGITS.iter().position(|d| d.eq_ignore_ascii_case(name)).map(|i| i.to_string())
}

fn mouse_of(name: &str) -> Option<MouseButton> {
    Some(match name {
        "LeftMouseButton" => MouseButton::Left,
        "RightMouseButton" => MouseButton::Right,
        "MiddleMouseButton" => MouseButton::Middle,
        "ThumbMouseButton" => MouseButton::Back,
        "ThumbMouseButton2" => MouseButton::Forward,
        _ => return None,
    })
}

/// Letter / digit keys by layout (UE3 virtual keys): characters held, by the physical key that
/// produced them.
#[derive(Default)]
pub struct Logical {
    held: HashMap<KeyCode, String>,
    just: HashSet<String>,
}

/// Takes the pressed / just-pressed state of every bound command.
#[allow(clippy::too_many_arguments)]
pub fn snapshot(
    keys: Res<ButtonInput<KeyCode>>,
    mut kev: EventReader<bevy::input::keyboard::KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: EventReader<MouseWheel>,
    mut capture: ResMut<Capture>,
    mut logical: Local<Logical>,
) {
    logical.just.clear();
    for ev in kev.read() {
        if ev.state.is_pressed() {
            if ev.repeat {
                continue;
            }
            if let Key::Character(c) = &ev.logical_key {
                let c = c.to_lowercase();
                logical.just.insert(c.clone());
                logical.held.insert(ev.key_code, c);
            }
        } else {
            logical.held.remove(&ev.key_code);
        }
    }
    // focus loss: keys released outside the window
    let held_codes: Vec<KeyCode> = logical.held.keys().copied().collect();
    for k in held_codes {
        if !keys.pressed(k) && !keys.just_released(k) {
            logical.held.remove(&k);
        }
    }
    let mut up = false;
    let mut dn = false;
    for w in wheel.read() {
        if w.y > 0.0 {
            up = true;
        } else if w.y < 0.0 {
            dn = true;
        }
    }
    let lchar = |c: &str, just: bool| if just { logical.just.contains(c) } else { logical.held.values().any(|x| x == c) };
    let state = |name: &str, just: bool| -> bool {
        if let Some(c) = char_of(name) {
            return lchar(&c, just);
        }
        if let Some(k) = named(name) {
            return if just { keys.just_pressed(k) } else { keys.pressed(k) };
        }
        if let Some(m) = mouse_of(name) {
            return if just { mouse.just_pressed(m) } else { mouse.pressed(m) };
        }
        match name {
            "MouseScrollUp" => up,
            "MouseScrollDown" => dn,
            _ => false,
        }
    };
    // options menu: the next key / button goes to the waiting binding
    if capture.waiting.is_some() {
        let mut got: Option<String> = None;
        for c in &logical.just {
            if c.len() == 1 && c.chars().all(|x| x.is_ascii_alphabetic()) {
                got = Some(c.to_uppercase());
            } else if let Some(d) = c.parse::<usize>().ok().filter(|d| *d < 10) {
                got = Some(DIGITS[d].to_string());
            }
        }
        if got.is_none() {
            for k in keys.get_just_pressed() {
                if *k == KeyCode::Escape {
                    got = Some(String::new());
                    break;
                }
                if let Some(n) = NAMED_LIST.iter().find(|n| named(n) == Some(*k)) {
                    got = Some(n.to_string());
                }
            }
        }
        if got.is_none() {
            for (n, m) in [("LeftMouseButton", MouseButton::Left), ("RightMouseButton", MouseButton::Right), ("MiddleMouseButton", MouseButton::Middle), ("ThumbMouseButton", MouseButton::Back), ("ThumbMouseButton2", MouseButton::Forward)] {
                if mouse.just_pressed(m) && !capture.skip_click {
                    got = Some(n.into());
                }
            }
            if up {
                got = Some("MouseScrollUp".into());
            } else if dn {
                got = Some("MouseScrollDown".into());
            }
        }
        capture.skip_click = false;
        if let Some(k) = got {
            capture.result = Some(k);
        }
    }
    let mut snap = Snapshot::default();
    if let Some(b) = BINDS.read().unwrap().as_ref() {
        for ((set, cmd), names) in &b.keys {
            if names.iter().any(|n| state(n, false)) {
                snap.down.insert((*set, cmd.clone()));
            }
            if names.iter().any(|n| state(n, true)) {
                snap.hit.insert((*set, cmd.clone()));
            }
        }
    }
    *SNAP.write().unwrap() = Some(snap);
}

/// Key names offered by capture (UE3 EKeys spelling).
const NAMED_LIST: &[&str] = &[
    "LeftShift", "RightShift", "LeftControl", "RightControl", "LeftAlt", "RightAlt", "SpaceBar", "Tab", "Enter", "BackSpace", "CapsLock", "Tilde", "Up", "Down", "Left", "Right", "Insert",
    "Delete", "Home", "End", "PageUp", "PageDown", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "NumPadZero", "NumPadOne", "NumPadTwo", "NumPadThree",
    "NumPadFour", "NumPadFive", "NumPadSix", "NumPadSeven", "NumPadEight", "NumPadNine", "Multiply", "Add", "Subtract", "Decimal", "Divide", "Semicolon", "Equals", "Comma", "Underscore",
    "Period", "Slash", "LeftBracket", "RightBracket", "Backslash", "Quote",
];

/// Options menu key capture: set `waiting`, read `result` ("" = Escape: cancelled).
#[derive(Resource, Default)]
pub struct Capture {
    pub waiting: Option<(Set, String, usize)>,
    pub result: Option<String>,
    /// the click that started the capture must not be taken as the new key
    pub skip_click: bool,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Capture>().add_systems(PreUpdate, snapshot.after(bevy::input::InputSystem));
    }
}

/// Marks the settings changed when a binding function returns (after its lock is released).
struct Touch;

impl Drop for Touch {
    fn drop(&mut self) {
        crate::settings::touch();
    }
}
