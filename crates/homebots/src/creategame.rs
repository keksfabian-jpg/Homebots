//! "Spiel erstellen" (PC_U_CreateMPGameScreen): the parts the game fills in by code.
//! Layout and look come from the scene; the values and behaviour here are measured in trace
//! frame 1000 (the user's create-game screen, 1366x768) or come from the game's data.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upk::ui::{GRef, ListRt, Scene, Ui, WidgetRt};

/// One entry of the available-map list.
pub struct MapEntry {
    pub display: String,
    pub desc: String,
    pub thumb: Option<GRef>,
    /// map folder (CookedPC/Maps/MP/<name>)
    pub folder: Option<PathBuf>,
    /// HomeBots can run it in the selected game mode (its gameplay sublevel exists, mapmode.rs)
    pub playable: bool,
    /// the folder has <map>_FrontlinesData (Bodenkontrolle) / TDM_<map> (Team Deathmatch)
    pub fl: bool,
    pub tdm: bool,
}

pub struct Slider {
    pub widget: usize,
    pub label: Option<usize>,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub nudge: f32,
}

/// What "Starten" hands to the game.
#[derive(Clone, Debug)]
pub struct MatchSettings {
    pub folder: PathBuf,
    pub game_type: String,
    /// GameModePrefix (FL = Bodenkontrolle, TDM, FL,BC / TDM,BC = Battle Commander)
    pub mode: String,
    /// slider values by widget tag (ScoreLimitSlider, RoundTimeSlider, ...)
    pub values: HashMap<String, f32>,
    /// check boxes by widget tag
    pub flags: HashMap<String, bool>,
}

pub struct CreateGame {
    pub maps: Vec<MapEntry>,
    pub avail: Option<usize>,
    pub queue_w: Option<usize>,
    pub thumb_w: Option<usize>,
    pub desc_w: Option<usize>,
    pub combo_edit: Option<usize>,
    pub combo: Option<usize>,
    pub sel: Option<usize>,
    pub hover: Option<usize>,
    pub queue: Vec<usize>,
    pub qsel: Option<usize>,
    pub game_types: Vec<String>,
    /// DefaultEngine.ini GameModeTypes GameModePrefix of each entry
    pub game_modes: Vec<&'static str>,
    pub game_type: usize,
    pub sliders: Vec<Slider>,
    /// check boxes (widget, checked, enabled)
    pub checks: Vec<(usize, bool, bool)>,
    pub radios: Vec<usize>,
    pub radio_sel: usize,
    pub edits: Vec<usize>,
    pub launch: Option<usize>,
    pub dragging: Option<usize>,
    /// ScoreLimitSlider (index in `sliders`), its scene range (value, min, max, nudge) for
    /// Bodenkontrolle and the TDM range (GCOnlineServerOptions m_TDMScoreLimit)
    pub score: Option<(usize, [f32; 4], Option<[f32; 4]>)>,
}

/// Map list of the create-game screen: every GCUIInfo.m_MPMapsInfo entry whose folder
/// (CookedPC/Maps/MP/<MapName>, case-insensitive) has a gameplay sublevel for at least one mode
/// (mapmode.rs; "playable" then follows the selected game type). m_MPMapsInfo (15 entries):
/// Basic, ANGELISLAND, BIGBOX, BORDERLANDS, CROSSROADS, FARM, HARBOR, LOWLANDS, SPILLWAY, SUBURB,
/// Culdesac, Bridge, MainStreet, Alcatraz, Mining. The user's install has no folder for Basic /
/// MainStreet / Mining, and Suburb only holds suburb_package.upk (no .map) -> not listed.
/// The game's own screen showed 8 of them in the trace (frame 1000: SPILLWAY, ANGELISLAND, FARM,
/// Bridge, HARBOR, CROSSROADS, Culdesac, LOWLANDS); HomeBots lists all installed maps (user
/// 2026-10-22 "port all multiplayer maps", NOT the original filter, whose rule is unknown).
/// Entries without MapTextureName (Bridge, Alcatraz) take UI_FrontEndTextures_DLC2.MPLoading.<MapName>
/// (that package has Alcatraz / Bridge / TDMSpillway / Overpass loading images).
///
/// Maps of the install that m_MPMapsInfo does not list (DLC maps): (folder name, localisation key
/// stem of ui_frontend.MapSelect.<key>_Name / _Text, thumbnail). Overpass = "HIGHWAY"
/// (ui_frontend.MapSelect.Overpass_Name, DLC2 texture UI_FrontEndTextures_DLC2.MPLoading.Overpass),
/// TDMSpillway = "WASSERWEG" (ui_frontend.MapSelect.TDMSpillway_Name, DLC2 texture
/// ...MPLoading.TDMSpillway; its own folder, Team Deathmatch only: all sublevels TDM_spillway_*).
const EXTRA_MAPS: [(&str, &str, &str); 2] = [("Overpass", "Overpass", "UI_FrontEndTextures_DLC2.MPLoading.Overpass"), ("TDMSpillway", "TDMSpillway", "UI_FrontEndTextures_DLC2.MPLoading.TDMSpillway")];

fn map_folder(game: &Path, name: &str) -> Option<PathBuf> {
    let mp = game.join("GCGame").join("CookedPC").join("Maps").join("MP");
    std::fs::read_dir(&mp).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
}

fn find_named(ui: &Ui, path: &str) -> Option<GRef> {
    ui.world.find(path)
}

impl CreateGame {
    pub fn new(ui: &Ui, scene: &Scene, game: &Path) -> CreateGame {
        let w = &ui.world;
        let by_tag = |t: &str| scene.widgets.iter().position(|x| x.tag == t);
        let mut maps = Vec::new();
        if let Some(info) = w.props(scene.widgets[0].obj).obj("UIInfoClass") {
            for m in w.structs(w.props(info).get("m_MPMapsInfo")) {
                let name = m.enum_i("MapName", 0).unwrap_or("").to_string();
                let thumb = m.str("MapTextureName").filter(|s| !s.is_empty()).and_then(|t| find_named(ui, t)).or_else(|| find_named(ui, &format!("UI_FrontEndTextures_DLC2.MPLoading.{name}")));
                let folder = map_folder(game, &name);
                maps.push(MapEntry {
                    display: ui.loc.resolve(m.str("MapDisplayName").unwrap_or("")),
                    desc: ui.loc.resolve(m.str("MapDescription").unwrap_or("")),
                    thumb,
                    fl: folder.as_deref().is_some_and(|f| crate::mapmode::has_data(f, "FL")),
                    tdm: folder.as_deref().is_some_and(|f| crate::mapmode::has_data(f, "TDM")),
                    folder,
                    playable: false,
                });
            }
        }
        for (dir, key, tex) in EXTRA_MAPS {
            let Some(folder) = map_folder(game, dir) else { continue };
            if maps.iter().any(|m| m.folder.as_ref() == Some(&folder)) {
                continue;
            }
            maps.push(MapEntry {
                display: ui.loc.resolve(&format!("<Strings:ui_frontend.MapSelect.{key}_Name>")),
                desc: ui.loc.resolve(&format!("<Strings:ui_frontend.MapSelect.{key}_Text>")),
                thumb: find_named(ui, tex),
                fl: crate::mapmode::has_data(&folder, "FL"),
                tdm: crate::mapmode::has_data(&folder, "TDM"),
                folder: Some(folder),
                playable: false,
            });
        }
        maps.retain(|m| m.folder.is_some() && (m.fl || m.tdm));
        maps.sort_by(|a, b| a.display.cmp(&b.display));

        // game modes: localised caps names (GameModes / *_Caps keys of the front-end strings)
        let caps = |key: &str| ui.loc.map.iter().find(|(k, _)| k.ends_with(&format!(".{key}"))).map(|(_, v)| v.clone());
        let mut game_modes: Vec<&'static str> = Vec::new();
        let mut game_types: Vec<String> = Vec::new();
        for (k, prefix) in [("gc_caps", "FL"), ("tdm_caps", "TDM"), ("bcgc_caps", "FL,BC"), ("bctdm_caps", "TDM,BC")] {
            if let Some(n) = caps(k) {
                game_types.push(n);
                game_modes.push(prefix);
            }
        }

        // HB_MODE=TDM / FL (GameModePrefix) and HB_MAP=<folder name>: preselect (tests,
        // HOMEBOTS_MENU_LAUNCH)
        let game_type = std::env::var("HB_MODE").ok().and_then(|m| game_modes.iter().position(|g| g.eq_ignore_ascii_case(&m))).unwrap_or(0);
        let mode = game_modes.get(game_type).copied().unwrap_or("FL");
        for m in &mut maps {
            m.playable = if crate::mapmode::is_tdm(mode) { m.tdm } else { m.fl };
        }
        let wanted = std::env::var("HB_MAP").ok().and_then(|w| maps.iter().position(|m| m.playable && m.folder.as_ref().and_then(|f| f.file_name()).is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&w))));
        let first_playable = wanted.or_else(|| maps.iter().position(|m| m.playable));

        let mut sliders = Vec::new();
        let mut checks = Vec::new();
        let mut edits = Vec::new();
        let index: HashMap<GRef, usize> = scene.widgets.iter().enumerate().map(|(i, x)| (x.obj, i)).collect();
        let radio_parent = by_tag("SelectTypeRadioButton");
        let mut radios = Vec::new();
        for (i, x) in scene.widgets.iter().enumerate() {
            if let Some(sv) = x.props.sub("SliderValue") {
                sliders.push(Slider {
                    widget: i,
                    label: x.props.obj("m_SettingsValueLabel").and_then(|g| index.get(&g).copied()),
                    value: sv.f32("CurrentValue").unwrap_or(0.0),
                    min: sv.f32("MinValue").unwrap_or(0.0),
                    max: sv.f32("MaxValue").unwrap_or(1.0),
                    nudge: sv.f32("NudgeValue").unwrap_or(1.0),
                });
            }
            if x.props.get("CheckedImageComponent").is_some() {
                if radio_parent.is_some() && x.parent == radio_parent {
                    radios.push(i);
                } else {
                    // trace frame 1000: Auto. Team-Ausgleich and Karten wiederholen checked;
                    // VAC has no meaning offline and stays off
                    let on = matches!(x.tag.as_str(), "AutoBalanceTeamsCheckBox" | "RepeatMapsCheckBox");
                    checks.push((i, on, x.tag != "VACCheckbox"));
                }
            }
            if x.class == "UIEditBox" && !x.tag.starts_with("Combo") {
                edits.push(i);
            }
        }
        // offline build: LAN
        let radio_sel = radios.iter().position(|&r| scene.widgets[r].tag == "LAN").unwrap_or(0);
        let combo = by_tag("GameTypeComboBox");
        let combo_edit = combo.and_then(|c| scene.widgets[c].props.obj("ComboEditbox")).and_then(|g| index.get(&g).copied());
        let score = sliders.iter().position(|s| scene.widgets[s.widget].tag == "ScoreLimitSlider").map(|i| {
            let s = &sliders[i];
            // "( CurrentVal=9000, MinRange=1000, MaxRange=30000, IncrementVal=1000 )"
            let tdm = ui.loc.ini.get("gcgame.gcgame.gconlineserveroptions.m_tdmscorelimit").and_then(|v| v.first()).and_then(|v| crate::tdm::range_value(v)).map(|r| [r.0, r.1, r.2, r.3]);
            (i, [s.value, s.min, s.max, s.nudge], tdm)
        });
        let mut cg = CreateGame {
            maps,
            avail: by_tag("AvailableMapList"),
            queue_w: by_tag("MapQueueList"),
            thumb_w: by_tag("MapThumb"),
            desc_w: by_tag("TextMapDescriptionTempLabel"),
            combo_edit,
            combo,
            sel: first_playable,
            hover: None,
            queue: first_playable.into_iter().collect(),
            qsel: None,
            game_types,
            game_modes,
            game_type,
            sliders,
            checks,
            radios,
            radio_sel,
            edits,
            launch: by_tag("LaunchButton"),
            dragging: None,
            score,
        };
        cg.apply_score_range();
        if wanted.is_none() && crate::mapmode::is_tdm(mode) {
            cg.mode_changed();
        }
        cg
    }

    /// Score limit slider: the scene's range for Bodenkontrolle, m_TDMScoreLimit for TDM.
    fn apply_score_range(&mut self) {
        let tdm = crate::mapmode::is_tdm(self.game_modes.get(self.game_type).copied().unwrap_or("FL"));
        if let Some((i, fl, t)) = self.score {
            let r = if tdm { t.unwrap_or(fl) } else { fl };
            let s = &mut self.sliders[i];
            (s.value, s.min, s.max, s.nudge) = (r[0], r[1], r[2], r[3]);
        }
    }

    pub fn runtime(&self, rt: &mut HashMap<usize, WidgetRt>, colors: [[f32; 4]; 2]) {
        if let Some(a) = self.avail {
            rt.entry(a).or_default().list = Some(ListRt {
                items: self.maps.iter().map(|m| (m.display.clone(), m.playable)).collect(),
                selected: self.sel,
                hover: self.hover,
                top: 0,
                colors,
            });
        }
        if let Some(q) = self.queue_w {
            rt.entry(q).or_default().list = Some(ListRt {
                items: self.queue.iter().map(|&i| (self.maps[i].display.clone(), true)).collect(),
                selected: self.qsel,
                hover: None,
                top: 0,
                colors,
            });
        }
        let shown = self.sel.or(self.queue.first().copied());
        if let (Some(t), Some(m)) = (self.thumb_w, shown) {
            rt.entry(t).or_default().image = self.maps[m].thumb;
        }
        if let Some(d) = self.desc_w {
            rt.entry(d).or_default().text = Some(shown.map(|m| self.maps[m].desc.clone()).unwrap_or_default());
        }
        if let Some(e) = self.combo_edit {
            rt.entry(e).or_default().text = self.game_types.get(self.game_type).cloned();
        }
        for s in &self.sliders {
            rt.entry(s.widget).or_default().slider = Some(s.value);
            if let Some(l) = s.label {
                rt.entry(l).or_default().text = Some(format!("{}", s.value.round() as i64));
            }
        }
        for &(w, on, _) in &self.checks {
            rt.entry(w).or_default().checked = Some(on);
        }
        for (i, &r) in self.radios.iter().enumerate() {
            rt.entry(r).or_default().checked = Some(i == self.radio_sel);
        }
        for &e in &self.edits {
            rt.entry(e).or_default().text = Some(String::new());
        }
    }

    fn row_at(scene: &Scene, w: usize, y: f32, vh: f32) -> Option<usize> {
        let p = &scene.widgets[w].props;
        let rh = p.f32("m_RowHeight").unwrap_or(21.0) * vh / 720.0;
        let r = scene.widgets[w].rect;
        (y >= r[1] && rh > 0.0).then(|| ((y - r[1]) / rh) as usize)
    }

    pub fn add_selected(&mut self) {
        if let Some(s) = self.sel {
            if self.maps[s].playable && !self.queue.contains(&s) {
                self.queue.push(s);
            }
        }
    }

    pub fn remove_selected(&mut self) {
        if let Some(q) = self.qsel {
            if q < self.queue.len() {
                self.queue.remove(q);
            }
            self.qsel = None;
        }
    }

    /// Mouse handling; returns true when the click was consumed.
    pub fn mouse(&mut self, scene: &Scene, c: bevy::math::Vec2, pressed: bool, held: bool, double: bool, vw: f32, vh: f32) -> bool {
        let inside = |w: usize| {
            let r = scene.widgets[w].rect;
            c.x >= r[0] && c.x < r[2] && c.y >= r[1] && c.y < r[3]
        };
        // slider drag
        if held {
            if let Some(si) = self.dragging {
                self.set_slider(scene, si, c.x, vw);
                return true;
            }
        } else {
            self.dragging = None;
        }
        self.hover = None;
        if let Some(a) = self.avail {
            if inside(a) {
                if let Some(row) = Self::row_at(scene, a, c.y, vh).filter(|&r| r < self.maps.len()) {
                    self.hover = Some(row);
                    if pressed {
                        self.sel = Some(row);
                        if double {
                            self.add_selected();
                        }
                        return true;
                    }
                }
            }
        }
        if !pressed {
            return false;
        }
        if let Some(q) = self.queue_w {
            if inside(q) {
                self.qsel = Self::row_at(scene, q, c.y, vh).filter(|&r| r < self.queue.len());
                if double {
                    self.remove_selected();
                }
                return true;
            }
        }
        for si in 0..self.sliders.len() {
            if inside(self.sliders[si].widget) {
                self.dragging = Some(si);
                self.set_slider(scene, si, c.x, vw);
                return true;
            }
        }
        for ch in &mut self.checks {
            if inside(ch.0) && ch.2 {
                ch.1 = !ch.1;
                return true;
            }
        }
        for (i, &r) in self.radios.iter().enumerate() {
            // the entry's label is part of the click area (its parent check box child)
            let lab = scene.widgets[r].children.iter().any(|&l| inside(l));
            if inside(r) || lab {
                self.radio_sel = i;
                return true;
            }
        }
        if let Some(cb) = self.combo {
            if inside(cb) && !self.game_types.is_empty() {
                self.game_type = (self.game_type + 1) % self.game_types.len();
                self.mode_changed();
                return true;
            }
        }
        false
    }

    /// Another game type: the maps with that mode's gameplay sublevel are playable (mapmode.rs);
    /// the queue keeps the playable ones, else gets the first playable map.
    fn mode_changed(&mut self) {
        let tdm = crate::mapmode::is_tdm(self.game_modes.get(self.game_type).copied().unwrap_or("FL"));
        for m in &mut self.maps {
            m.playable = if tdm { m.tdm } else { m.fl };
        }
        // the queue starts over with one map: TDM prefers a TDM-only map (Overpass), so
        // "Starten" right after picking TDM starts it (NOT from data: the game's queue rules)
        let pick = if tdm { self.maps.iter().position(|m| m.tdm && !m.fl) } else { None }.or_else(|| self.sel.filter(|&i| self.maps[i].playable)).or_else(|| self.maps.iter().position(|m| m.playable));
        self.queue = pick.into_iter().collect();
        self.sel = pick;
        self.qsel = None;
        self.apply_score_range();
    }

    /// Marker centre follows the mouse; value snaps to NudgeValue (bIntRange sliders).
    fn set_slider(&mut self, scene: &Scene, si: usize, x: f32, vw: f32) {
        let s = &mut self.sliders[si];
        let w = &scene.widgets[s.widget];
        let mw = w.props.sub("MarkerWidth").and_then(|b| b.f32("Value")).unwrap_or(0.02) * vw;
        let span = (w.rect[2] - w.rect[0] - mw).max(1.0);
        let t = ((x - w.rect[0] - mw * 0.5) / span).clamp(0.0, 1.0);
        let mut v = s.min + t * (s.max - s.min);
        if s.nudge > 0.0 {
            v = s.min + ((v - s.min) / s.nudge).round() * s.nudge;
        }
        s.value = v.clamp(s.min, s.max);
    }

    pub fn settings(&self, scene: &Scene) -> Option<MatchSettings> {
        let first = *self.queue.first()?;
        let m = &self.maps[first];
        if !m.playable {
            return None;
        }
        Some(MatchSettings {
            folder: m.folder.clone()?,
            game_type: self.game_types.get(self.game_type).cloned().unwrap_or_default(),
            mode: self.game_modes.get(self.game_type).copied().unwrap_or("FL").to_string(),
            values: self.sliders.iter().map(|s| (scene.widgets[s.widget].tag.clone(), s.value)).collect(),
            flags: self.checks.iter().map(|c| (scene.widgets[c.0].tag.clone(), c.1)).collect(),
        })
    }
}
