//! The options screens (U_Options: PC_U_GraphicSettings "System", U_HUDPrefs "Spiel",
//! PC_U_KeyBindings "Steuerung"): every option widget is bound by its data-store markup
//! (DataSource / ValueDataSource / CaptionDataSource MarkupString) to settings.rs, key binding
//! fields to input.rs. Layout and look come from the scenes; combo box entries from the loc
//! files (gcgame.gcuidataprovider_*.m_<field>[i], GCUIObject_DifficultyOptionSelect
//! m_DifficultySettings), resolutions from the monitor.
//!
//! The settings take effect at once except window mode and resolution, which wait for
//! "Übernehmen" (ApplyChangesButton) as in the game. Leaving a screen saves.
use bevy::prelude::*;
use std::collections::HashMap;
use upk::ui::{GRef, Scene, Ui, WidgetRt};

pub const SCENES: [&str; 3] = ["PC_U_GraphicSettings", "U_HUDPrefs", "PC_U_KeyBindings"];

struct OSlider {
    widget: usize,
    key: String,
    min: f32,
    max: f32,
    nudge: f32,
    label: Option<usize>,
}

struct OCombo {
    widget: usize,
    edit: Option<usize>,
    key: String,
    items: Vec<String>,
}

struct OKey {
    widget: usize,
    set: crate::input::Set,
    cmd: String,
    slot: usize,
}

pub struct Options {
    sliders: Vec<OSlider>,
    checks: Vec<(usize, String)>,
    combos: Vec<OCombo>,
    keys: Vec<OKey>,
    /// tab buttons and their pages
    tabs: Vec<(usize, usize)>,
    tab: usize,
    /// open combo list: combo index, hovered row
    open: Option<(usize, Option<usize>)>,
    dragging: Option<usize>,
    /// key field waiting for a key
    waiting: Option<usize>,
    /// the settings keys of this screen (ResetToDefaults)
    prefixes: Vec<String>,
    /// key names (UI_Keybinds.Keybinds.<name>)
    key_names: HashMap<String, String>,
    pub resets_binds: bool,
    /// label texts replaced at runtime (widget, text)
    relabel: Vec<(usize, String)>,
}

fn markup(w: &upk::ui::Widget) -> Option<String> {
    let mk = |n: &str| w.props.sub(n).and_then(|t| t.str("MarkupString").map(String::from));
    let m = mk("DataSource").or_else(|| mk("ValueDataSource")).or_else(|| mk("CaptionDataSource"))?;
    let m = m.trim().trim_start_matches('<').trim_end_matches('>').to_string();
    (m.contains(':') && !m.starts_with("Strings:")).then_some(m)
}

/// Combo entries of a data-store field from the loc files.
fn combo_items(ui: &Ui, key: &str) -> Vec<String> {
    let field = key.split(':').nth(1).unwrap_or("").to_ascii_lowercase();
    let pat = if key == "PlayerDetails:Difficulty" { "gcgame.gcuiobject_difficultyoptionselect.m_difficultysettings[".to_string() } else { format!(".m_{field}[") };
    let mut v: Vec<(usize, String)> = ui
        .loc
        .map
        .iter()
        .filter(|(k, _)| (key == "PlayerDetails:Difficulty" || k.starts_with("gcgame.gcuidataprovider_")) && k.contains(&pat))
        .filter_map(|(k, val)| {
            let i = k.rsplit_once('[')?.1.trim_end_matches(']').parse::<usize>().ok()?;
            Some((i, ui.loc.resolve(val)))
        })
        .collect();
    v.sort();
    v.into_iter().map(|x| x.1).collect()
}

impl Options {
    pub fn new(ui: &Ui, scene: &Scene) -> Options {
        let index: HashMap<GRef, usize> = scene.widgets.iter().enumerate().map(|(i, x)| (x.obj, i)).collect();
        let mut o = Options { sliders: Vec::new(), checks: Vec::new(), combos: Vec::new(), keys: Vec::new(), tabs: Vec::new(), tab: 0, open: None, dragging: None, waiting: None, prefixes: Vec::new(), key_names: HashMap::new(), resets_binds: false, relabel: Vec::new() };
        for (i, w) in scene.widgets.iter().enumerate() {
            let m = markup(w);
            if let (Some(sv), Some(k)) = (w.props.sub("SliderValue"), m.as_ref()) {
                o.sliders.push(OSlider {
                    widget: i,
                    key: k.clone(),
                    min: sv.f32("MinValue").unwrap_or(0.0),
                    max: sv.f32("MaxValue").unwrap_or(1.0),
                    nudge: sv.f32("NudgeValue").unwrap_or(0.0),
                    label: w.props.obj("m_SettingsValueLabel").and_then(|g| index.get(&g).copied()),
                });
                continue;
            }
            if w.class.to_ascii_lowercase().contains("checkbox") {
                if let Some(k) = m {
                    // the Logitech LCD row (no LCD support here) is the HomeBots option
                    // "Zufälliges Wetter/Tageszeit" (weather.rs; NOT from data, user 2026-10-22)
                    if k == "PlayerDetails:LogitechLCD" {
                        o.checks.push((i, crate::weather::RANDOM_KEY.to_string()));
                        if let Some(l) = w.parent {
                            o.relabel.push((l, "Zufälliges Wetter/Tageszeit".to_string()));
                        }
                    } else {
                        o.checks.push((i, k));
                    }
                }
                continue;
            }
            if w.class == "UIComboBox" {
                let edit = w.props.obj("ComboEditbox").and_then(|g| index.get(&g).copied());
                let list = w.props.obj("ComboList").and_then(|g| index.get(&g).copied());
                let key = list.and_then(|l| markup(&scene.widgets[l]));
                if let Some(key) = key {
                    let items = if key == "GraphicsSettings:ScreenResolution" { Vec::new() } else { combo_items(ui, &key) };
                    o.combos.push(OCombo { widget: i, edit, key, items });
                }
                continue;
            }
            if w.class.contains("KeyBindPrimary") || w.class.contains("KeyBindSecondary") {
                if let Some(k) = m.as_ref().and_then(|k| k.strip_prefix("InputActionToKey:")) {
                    if let Some((set, cmd)) = crate::input::resolve(k) {
                        o.keys.push(OKey { widget: i, set, cmd, slot: if w.class.contains("Secondary") { 1 } else { 0 } });
                    }
                }
                continue;
            }
            if w.class == "UITabButton" {
                if let Some(&page) = w.children.iter().find(|&&c| scene.widgets[c].class == "UITabPage") {
                    o.tabs.push((i, page));
                }
            }
        }
        o.resets_binds = !o.keys.is_empty();
        let mut keys: Vec<String> = o.sliders.iter().map(|s| s.key.clone()).chain(o.checks.iter().map(|c| c.1.clone())).chain(o.combos.iter().map(|c| c.key.clone())).collect();
        if o.combos.iter().any(|c| c.key == "GraphicsSettings:ScreenResolution") {
            keys.extend(["GraphicsSettings:ResX".to_string(), "GraphicsSettings:ResY".to_string()]);
        }
        o.prefixes = keys;
        o.tab = o.tabs.iter().position(|&(_, p)| !scene.widgets[p].hidden).unwrap_or(0);
        for (k, v) in &ui.loc.map {
            if let Some(n) = k.strip_prefix("ui_keybinds.keybinds.") {
                o.key_names.insert(n.to_string(), v.clone());
            }
        }
        info!("options: {} sliders, {} check boxes, {} combo boxes, {} key fields, {} tabs", o.sliders.len(), o.checks.len(), o.combos.len(), o.keys.len(), o.tabs.len());
        o
    }

    fn combo_index(&self, c: &OCombo) -> Option<usize> {
        if c.key == "GraphicsSettings:ScreenResolution" {
            let (w, h) = (crate::settings::get("GraphicsSettings:ResX") as u32, crate::settings::get("GraphicsSettings:ResY") as u32);
            return crate::settings::resolutions().iter().position(|&r| r == (w, h));
        }
        let v = crate::settings::get(&c.key).round();
        (v >= 0.0).then_some(v as usize)
    }

    fn items(&self, c: &OCombo) -> Vec<String> {
        if c.key == "GraphicsSettings:ScreenResolution" {
            return crate::settings::resolutions().iter().map(|(w, h)| format!("{w} x {h}")).collect();
        }
        c.items.clone()
    }

    fn key_text(&self, name: &str) -> String {
        self.key_names.get(&name.to_ascii_lowercase()).cloned().unwrap_or_else(|| name.to_string())
    }

    pub fn runtime(&self, rt: &mut HashMap<usize, WidgetRt>, start_size: (f32, f32)) {
        for s in &self.sliders {
            let mut v = crate::settings::get(&s.key);
            // FieldOfView 0 = the measured view: shown as its horizontal angle
            if s.key == "PlayerDetails:FieldOfView" && v < 1.0 {
                v = (2.0 * ((crate::player::VFOV_DEG.to_radians() * 0.5).tan() * (start_size.0 / start_size.1.max(1.0))).atan()).to_degrees().round();
            }
            rt.entry(s.widget).or_default().slider = Some(v.clamp(s.min, s.max));
            if let Some(l) = s.label {
                let int = s.nudge >= 1.0 || s.max - s.min > 20.0;
                rt.entry(l).or_default().text = Some(if int { format!("{}", v.round() as i64) } else { format!("{v:.1}") });
            }
        }
        for (w, k) in &self.checks {
            rt.entry(*w).or_default().checked = Some(crate::settings::flag(k));
        }
        for (w, t) in &self.relabel {
            rt.entry(*w).or_default().text = Some(t.clone());
        }
        for c in &self.combos {
            if let Some(e) = c.edit {
                let items = self.items(c);
                let text = match self.combo_index(c) {
                    Some(i) => items.get(i).cloned().unwrap_or_default(),
                    None if c.key == "GraphicsSettings:ScreenResolution" => format!("{} x {}", start_size.0 as u32, start_size.1 as u32),
                    None => String::new(),
                };
                rt.entry(e).or_default().text = Some(text);
            }
        }
        for (ki, k) in self.keys.iter().enumerate() {
            let text = if self.waiting == Some(ki) {
                "...".to_string()
            } else {
                crate::input::keys_of(k.set, &k.cmd).get(k.slot).map(|n| self.key_text(n)).unwrap_or_default()
            };
            rt.entry(k.widget).or_default().text = Some(text);
        }
        for (ti, &(_, page)) in self.tabs.iter().enumerate() {
            rt.entry(page).or_default().hidden = Some(ti != self.tab);
        }
    }

    /// Takes a captured key (input.rs) into the waiting field.
    pub fn poll(&mut self, cap: &mut crate::input::Capture) {
        let Some(ki) = self.waiting else { return };
        if let Some(k) = cap.result.take() {
            let f = &self.keys[ki];
            if !k.is_empty() {
                crate::input::bind(f.set, &f.cmd, f.slot, &k);
                info!("options: {:?}.{} slot {} = {k}", f.set, f.cmd, f.slot);
            }
            self.waiting = None;
            cap.waiting = None;
        }
    }

    pub fn capturing(&self) -> bool {
        self.waiting.is_some()
    }

    fn row_h(vh: f32) -> f32 {
        22.0 * vh / 720.0
    }

    /// Open list rect (x0, y0, x1, y1) and rows.
    fn list_rect(&self, scene: &Scene, ci: usize, vh: f32) -> ([f32; 4], usize) {
        let c = &self.combos[ci];
        let r = scene.widgets[c.widget].rect;
        let n = self.items(c).len();
        let h = n as f32 * Self::row_h(vh);
        let (y0, y1) = if r[3] + h <= vh { (r[3], r[3] + h) } else { ((r[1] - h).max(0.0), r[1]) };
        ([r[0], y0, r[2], y1], n)
    }

    /// Mouse handling; true when consumed.
    #[allow(clippy::too_many_arguments)]
    pub fn mouse(&mut self, scene: &Scene, c: Vec2, pressed: bool, held: bool, rpressed: bool, vw: f32, vh: f32, cap: &mut crate::input::Capture) -> bool {
        let inside = |r: [f32; 4]| c.x >= r[0] && c.x < r[2] && c.y >= r[1] && c.y < r[3];
        if self.waiting.is_some() {
            return true;
        }
        if let Some((ci, _)) = self.open {
            let (r, n) = self.list_rect(scene, ci, vh);
            let row = inside(r).then(|| (((c.y - r[1]) / Self::row_h(vh)) as usize).min(n.saturating_sub(1)));
            self.open = Some((ci, row));
            if pressed {
                if let Some(row) = row {
                    self.choose(ci, row);
                }
                self.open = None;
            }
            return true;
        }
        if held {
            if let Some(si) = self.dragging {
                self.set_slider(scene, si, c.x, vw);
                return true;
            }
        } else {
            self.dragging = None;
        }
        if rpressed {
            // right click clears a key field
            if let Some(k) = self.keys.iter().find(|k| inside(scene.widgets[k.widget].rect) && self.visible(scene, k.widget)) {
                crate::input::unbind(k.set, &k.cmd, k.slot);
                return true;
            }
        }
        if !pressed {
            return false;
        }
        for ti in 0..self.tabs.len() {
            if inside(scene.widgets[self.tabs[ti].0].rect) && !inside(scene.widgets[self.tabs[ti].1].rect) {
                self.tab = ti;
                return true;
            }
        }
        for si in 0..self.sliders.len() {
            if inside(scene.widgets[self.sliders[si].widget].rect) && self.visible(scene, self.sliders[si].widget) {
                self.dragging = Some(si);
                self.set_slider(scene, si, c.x, vw);
                return true;
            }
        }
        for (w, k) in &self.checks {
            if inside(scene.widgets[*w].rect) && self.visible(scene, *w) {
                crate::settings::set(k, if crate::settings::flag(k) { 0.0 } else { 1.0 });
                return true;
            }
        }
        for ci in 0..self.combos.len() {
            let w = self.combos[ci].widget;
            if inside(scene.widgets[w].rect) && self.visible(scene, w) && !self.items(&self.combos[ci]).is_empty() {
                self.open = Some((ci, None));
                return true;
            }
        }
        for ki in 0..self.keys.len() {
            let w = self.keys[ki].widget;
            if inside(scene.widgets[w].rect) && self.visible(scene, w) {
                self.waiting = Some(ki);
                cap.waiting = Some((self.keys[ki].set, self.keys[ki].cmd.clone(), self.keys[ki].slot));
                cap.result = None;
                cap.skip_click = true;
                return true;
            }
        }
        false
    }

    /// The widget and its parents are shown (inactive tab pages are hidden).
    fn visible(&self, scene: &Scene, mut w: usize) -> bool {
        loop {
            if let Some(ti) = self.tabs.iter().position(|&(_, p)| p == w) {
                if ti != self.tab {
                    return false;
                }
            } else if scene.widgets[w].hidden {
                return false;
            }
            match scene.widgets[w].parent {
                Some(p) => w = p,
                None => return true,
            }
        }
    }

    fn choose(&mut self, ci: usize, row: usize) {
        let key = self.combos[ci].key.clone();
        if key == "GraphicsSettings:ScreenResolution" {
            if let Some(&(w, h)) = crate::settings::resolutions().get(row) {
                crate::settings::set("GraphicsSettings:ResX", w as f32);
                crate::settings::set("GraphicsSettings:ResY", h as f32);
            }
            return;
        }
        crate::settings::set(&key, row as f32);
        // OverallQuality: a preset sets the four quality entries to its level (NOT from data: the
        // game's preset table is native); changing one of them makes it "Custom"
        const QUALITY: [&str; 4] = ["GraphicsSettings:TextureQuality", "GraphicsSettings:ShadowQuality", "GraphicsSettings:ShaderQuality", "GraphicsSettings:WorldDetail"];
        if key == "GraphicsSettings:OverallQuality" && row < 5 {
            for q in QUALITY {
                crate::settings::set(q, row as f32);
            }
        } else if QUALITY.contains(&key.as_str()) {
            crate::settings::set("GraphicsSettings:OverallQuality", 5.0);
        }
        if key == "GraphicsSettings:ScreenAspectRatios" {
            // the resolution list follows the aspect: keep the size if it fits, else the largest
            let list = crate::settings::resolutions();
            let cur = (crate::settings::get("GraphicsSettings:ResX") as u32, crate::settings::get("GraphicsSettings:ResY") as u32);
            if !list.contains(&cur) {
                if let Some(&(w, h)) = list.last() {
                    crate::settings::set("GraphicsSettings:ResX", w as f32);
                    crate::settings::set("GraphicsSettings:ResY", h as f32);
                }
            }
        }
    }

    /// Marker centre follows the mouse; the value snaps to NudgeValue.
    fn set_slider(&mut self, scene: &Scene, si: usize, x: f32, vw: f32) {
        let s = &self.sliders[si];
        let w = &scene.widgets[s.widget];
        let mw = w.props.sub("MarkerWidth").and_then(|b| b.f32("Value")).unwrap_or(0.02) * vw;
        let span = (w.rect[2] - w.rect[0] - mw).max(1.0);
        let t = ((x - w.rect[0] - mw * 0.5) / span).clamp(0.0, 1.0);
        let mut v = s.min + t * (s.max - s.min);
        if s.nudge > 0.0 {
            v = s.min + ((v - s.min) / s.nudge).round() * s.nudge;
        }
        let v = (v.clamp(s.min, s.max) * 1000.0).round() / 1000.0;
        crate::settings::set(&s.key, v);
    }

    /// ResetToDefaults: this screen's settings (and key bindings) back to the defaults.
    pub fn reset(&mut self) {
        crate::settings::reset_keys(&self.prefixes);
        if self.resets_binds {
            crate::input::reset();
        }
        info!("options: reset {} settings", self.prefixes.len());
    }

    /// The open combo list, drawn over the scene: dark rows, the current entry and the hovered
    /// one highlighted (the UIList style of the scene is not drawn; look NOT from data).
    #[allow(clippy::too_many_arguments)]
    pub fn overlay(&self, ui: &Ui, scene: &Scene, font: Option<GRef>, white: Option<GRef>, colors: [[f32; 4]; 2], vw: f32, vh: f32, out: &mut Vec<upk::ui::Quad>) {
        let Some((ci, hover)) = self.open else { return };
        let (r, n) = self.list_rect(scene, ci, vh);
        let c = &self.combos[ci];
        let items = self.items(c);
        let sel = self.combo_index(c);
        let rh = Self::row_h(vh);
        out.push(upk::ui::Quad { rot: 0.0, rect: r, uv: [0.0, 0.0, 1.0, 1.0], tex: white, color: [0.02, 0.02, 0.02, 1.0] });
        let k = vh / 720.0;
        for (i, t) in items.iter().enumerate().take(n) {
            let y = r[1] + i as f32 * rh;
            if hover == Some(i) {
                out.push(upk::ui::Quad { rot: 0.0, rect: [r[0], y, r[2], y + rh], uv: [0.0, 0.0, 1.0, 1.0], tex: white, color: [0.25, 0.25, 0.25, 0.95] });
            }
            let col = if sel == Some(i) { colors[1] } else { colors[0] };
            ui.text_px(t, font, [r[0] + 6.0 * k, y + 2.0 * k], 0.62 * k, col, vw, vh, out);
        }
    }
}

/// Applies the settings that act every frame or on change: window (mode, size: on
/// "Übernehmen" and at start; vsync), gamma and bloom of the post chain, the frame rate shown.
#[allow(clippy::too_many_arguments)]
fn apply(
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    monitors: Query<&bevy::window::Monitor>,
    mut posts: Query<(Entity, &mut crate::hfpost::HfPost)>,
    time: Res<Time<Real>>,
    mut seen: Local<(u64, u64, Option<(f32, f32)>)>,
    mut bloom: Local<HashMap<Entity, (f32, f32)>>,
    mut fps: Local<f32>,
    mut dirty_at: Local<Option<f64>>,
) {
    let dt = time.delta_secs();
    if dt > 0.0 {
        *fps = if *fps <= 0.0 { 1.0 / dt } else { *fps * 0.95 + (1.0 / dt) * 0.05 };
        crate::settings::set_fps(*fps);
    }
    if crate::settings::MODES.read().unwrap().is_empty() {
        let mut modes: Vec<bevy::window::VideoMode> = monitors.iter().flat_map(|m| m.video_modes.iter().cloned()).collect();
        if !modes.is_empty() {
            modes.sort_by_key(|m| (m.physical_size.x, m.physical_size.y, std::cmp::Reverse(m.refresh_rate_millihertz)));
            *crate::settings::MODES.write().unwrap() = modes;
        }
    }
    let Ok(mut w) = windows.single_mut() else { return };
    if seen.2.is_none() {
        seen.2 = Some((w.resolution.physical_width() as f32, w.resolution.physical_height() as f32));
    }
    START_SIZE.write().unwrap().replace(seen.2.unwrap());
    let ver = crate::settings::version();
    if ver != seen.0 {
        // HB_VSYNC=0 (profiling) wins over the option
        let vsync = crate::settings::flag("GraphicsSettings:LockToVSync") && !std::env::var("HB_VSYNC").is_ok_and(|v| v == "0");
        let pm = if vsync { bevy::window::PresentMode::AutoVsync } else { bevy::window::PresentMode::AutoNoVsync };
        if w.present_mode != pm && std::env::var_os("HB_VSYNC").is_none() {
            w.present_mode = pm;
        }
    }
    let epoch = crate::settings::window_epoch();
    if epoch != seen.1 && std::env::var_os("HOMEBOTS_SIZE").is_none() {
        let (rx, ry) = (crate::settings::get("GraphicsSettings:ResX") as u32, crate::settings::get("GraphicsSettings:ResY") as u32);
        let mode = crate::settings::get("GraphicsSettings:FullscreenMode").round() as i32;
        use bevy::window::{MonitorSelection, WindowMode};
        let want = match mode {
            // Vollbild: borderless over the whole monitor. Exclusive fullscreen
            // (WindowMode::Fullscreen with the monitor's video mode) lost the GPU device on the
            // user's AMD Radeon iGPU right at start ("Surface::configure: Parent device is lost",
            // 2026-10-19 22:16); before, VideoModeSelection::Current was refused by winit and
            // "Übernehmen" did nothing. NOT from data (the game switches the display mode).
            1 => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            2 => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
        if w.mode != want {
            info!("options: window mode {want:?}");
            w.mode = want;
        }
        // windowed without a chosen size: the start size (back from fullscreen)
        let (rx, ry) = if mode == 0 && (rx == 0 || ry == 0) { seen.2.map(|s| (s.0 as u32, s.1 as u32)).unwrap_or((rx, ry)) } else { (rx, ry) };
        if mode == 0 && rx > 0 && ry > 0 && (w.resolution.physical_width() != rx || w.resolution.physical_height() != ry) {
            info!("options: window {rx} x {ry}");
            w.resolution.set_physical_resolution(rx, ry);
        }
    }
    // save a second after the last change (also when the game is closed without "Zurück")
    if ver != seen.0 && seen.0 != 0 {
        *dirty_at = Some(time.elapsed_secs_f64());
    }
    if dirty_at.is_some_and(|t| time.elapsed_secs_f64() - t > 1.0) {
        *dirty_at = None;
        crate::settings::save();
    }
    seen.0 = ver;
    seen.1 = epoch;
    // gamma (Display Gamma, 2.2 = the game's) and bloom on / off
    let gamma = crate::settings::get("GraphicsSettings:Gamma").clamp(0.5, 5.0);
    let bloom_on = crate::settings::flag("GraphicsSettings:Bloom");
    for (e, mut p) in &mut posts {
        // (level bloom, value written): a new level's post settings bring their own bloom
        let ent = bloom.entry(e).or_insert((p.misc.z, p.misc.z));
        if p.misc.z != ent.1 {
            *ent = (p.misc.z, p.misc.z);
        }
        let b = if bloom_on { ent.0 } else { 0.0 };
        if p.misc.z != b {
            p.misc.z = b;
        }
        ent.1 = b;
        let g = 1.0 / gamma;
        if (p.gamma.w - g).abs() > 1e-5 {
            p.gamma.w = g;
        }
    }
}

/// The window's size at start (the "current" resolution entry while none is chosen).
pub static START_SIZE: std::sync::RwLock<Option<(f32, f32)>> = std::sync::RwLock::new(None);

pub fn start_size() -> (f32, f32) {
    START_SIZE.read().unwrap().unwrap_or((1366.0, 768.0))
}

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply);
    }
}
