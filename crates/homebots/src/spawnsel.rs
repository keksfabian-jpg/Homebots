//! PC_U_SpawnSelectionMenu (GCUIScene_SpawnSelection): loadout and vehicle choice before
//! spawning. The scene's labels are filled at runtime (trace t3 frame 1500 shows the values):
//! - loadouts: GCGame.ini [GCCustomLoadoutProfiles] DefaultLoadouts (names "ANGRIFF", "MP", ...),
//!   item names from GCLoadoutItemInfos.LoadoutItemInfos, abilities from .AbilityInfos;
//! - vehicles: "INFANTERIE", "TEAM-FAHRZEUG" (ui_frontend GCUIScene_SpawnSelection), then
//!   GCBattlePoints.ini VehicleCosts as "%d - KP" + VehicleNameString, sorted by cost;
//! - battle points: GCBattlePoints.ini InitialBattlePoints, special item costs SpecialAbilityCosts;
//! - auto spawn countdown: GCGame.ini [PlayerSpawnManagerMP] SpawnMenuAutoSpawnTime.
use std::collections::HashMap;
use upk::ui::{Scene, Ui, WidgetRt};

#[derive(Clone, Debug, Default)]
pub struct Loadout {
    pub name: String,
    /// CFG_ class names: primary weapon first, then the other inventory groups
    pub items: Vec<String>,
    pub attachments: Vec<String>,
    pub abilities: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct VehicleEntry {
    pub label: String,
    pub cost: Option<i32>,
    /// class name (GCV_US_..._Content), None for infantry / team vehicle
    pub class: Option<String>,
    pub enabled: bool,
}

pub struct SpawnSel {
    pub loadouts: Vec<Loadout>,
    pub vehicles: Vec<VehicleEntry>,
    pub sel_loadout: usize,
    pub sel_vehicle: usize,
    pub hover: Option<(u8, usize)>,
    /// SpawnMenuAutoSpawnTime and the menu time counted so far (frame deltas clamped, so a
    /// frame that blocks on the level load or a pipeline compile does not eat the countdown)
    pub auto: f64,
    pub elapsed: f64,
    pub battle_points: i32,
    /// opened from the pause menu while alive ("KLASSE WECHSELN"): no countdown, the choice
    /// applies to the next spawn
    pub alive: bool,
    /// the player's team (combat::player_team) when the menu was built: flag and vehicles
    pub team: u8,
    item_names: HashMap<String, String>,
    item_kind: HashMap<String, String>,
    item_image: HashMap<String, upk::ui::GRef>,
    ability_names: HashMap<String, String>,
    ability_icon: HashMap<String, upk::ui::GRef>,
    /// EquipedAbilities simple objects: (label index, icon index) in fill order
    ability_slots: Vec<(usize, Option<usize>)>,
    special_cost: HashMap<String, i32>,
    tags: HashMap<String, usize>,
}

/// Values of `Key=` in a UE struct literal like "(A=1, B=(x,y))" (top-level only).
pub fn ue_field<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let mut depth = 0i32;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 1 && s[i..].len() > key.len() && s[i..i + key.len()].eq_ignore_ascii_case(key) && (i == 0 || !(b[i - 1] as char).is_alphanumeric() && b[i - 1] != b'_') {
            let rest = s[i + key.len()..].trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim_start();
                let off = s.len() - rest.len();
                // value: balanced parentheses or up to , / )
                let rb = rest.as_bytes();
                let mut d = 0i32;
                let mut j = 0;
                let mut in_q = false;
                while j < rb.len() {
                    match rb[j] {
                        b'"' => in_q = !in_q,
                        b'(' if !in_q => d += 1,
                        b')' if !in_q => {
                            if d == 0 {
                                break;
                            }
                            d -= 1;
                        }
                        b',' if !in_q && d == 0 => break,
                        _ => {}
                    }
                    j += 1;
                }
                return Some(s[off..off + j].trim());
            }
        }
        i += 1;
    }
    None
}

/// Elements of a parenthesised list "(a,b,(c))".
pub fn ue_list(s: &str) -> Vec<String> {
    let s = s.trim();
    let inner = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(s);
    let mut out = Vec::new();
    let mut d = 0;
    let mut cur = String::new();
    for c in inner.chars() {
        match c {
            '(' => {
                d += 1;
                cur.push(c)
            }
            ')' => {
                d -= 1;
                cur.push(c)
            }
            ',' if d == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

impl SpawnSel {
    pub fn new(ui: &Ui, scene: &Scene, _now: f64) -> SpawnSel {
        let loc = &ui.loc;
        let ini = |k: &str| loc.ini.get(&k.to_ascii_lowercase()).cloned().unwrap_or_default();
        let mut loadouts = Vec::new();
        for l in ini("gcgame.gcgame.gccustomloadoutprofiles.defaultloadouts") {
            let name = ue_field(&l, "Name").map(|n| loc.resolve(n.trim_matches('"'))).unwrap_or_default();
            let mut lo = Loadout { name, ..Default::default() };
            if let Some(groups) = ue_field(&l, "InventoryGroups") {
                for g in ue_list(groups) {
                    if let Some(p) = ue_field(&g, "PrimaryItem") {
                        lo.items.push(p.to_string());
                    }
                    if let Some(sec) = ue_field(&g, "SecondaryItems") {
                        lo.attachments.extend(ue_list(sec));
                    }
                }
            }
            if let Some(a) = ue_field(&l, "InfAbilities") {
                lo.abilities = ue_list(a);
            }
            loadouts.push(lo);
        }
        // item / ability display names
        let mut item_names = HashMap::new();
        let mut item_kind = HashMap::new();
        let mut ability_names = HashMap::new();
        let mut item_image = HashMap::new();
        let mut ability_icon = HashMap::new();
        if let Some(r) = ui.world.find("GCLoadoutItemInfos.LoadoutItemInfos") {
            for t in ui.world.structs(ui.world.props(r).get("ItemInfos")) {
                if let (Some(c), Some(n)) = (t.str("ItemClassName").map(String::from).or_else(|| t.enum_i("ItemClassName", 0).map(String::from)), t.str("strItemDisplayName")) {
                    item_names.insert(c.to_ascii_lowercase(), loc.resolve(n));
                    item_kind.insert(c.to_ascii_lowercase(), t.enum_i("InventoryType", 0).unwrap_or("").to_string());
                    if let Some(g) = t.obj("ItemImage").and_then(|g| ui.world.gref(g.pkg, g.idx)).filter(|g| g.idx > 0) {
                        item_image.insert(c.to_ascii_lowercase(), g);
                    }
                }
            }
        }
        if let Some(r) = ui.world.find("GCLoadoutItemInfos.AbilityInfos") {
            for t in ui.world.structs(ui.world.props(r).get("AbilityInfos")) {
                if let (Some(c), Some(n)) = (t.str("AbilityName").map(String::from).or_else(|| t.enum_i("AbilityName", 0).map(String::from)), t.str("strDisplayName")) {
                    ability_names.insert(c.to_ascii_lowercase(), loc.resolve(n));
                    if let Some(g) = t.obj("SmallIcon").and_then(|g| ui.world.gref(g.pkg, g.idx)).filter(|g| g.idx > 0) {
                        ability_icon.insert(c.to_ascii_lowercase(), g);
                    }
                }
            }
        }
        // ability rows: labels column by column, each with the icon on its row left of it
        let mut ability_slots = Vec::new();
        if let Some(w) = scene.widgets.iter().find(|w| w.tag == "EquipedAbilities") {
            let objs = ui.world.objects(w.props.get("SimpleObjects"));
            let pos = |o: upk::ui::GRef| ui.world.props(o).floats("Position", 2).unwrap_or(vec![0.0, 0.0]);
            let mut labels: Vec<(usize, Vec<f32>)> = objs.iter().enumerate().filter(|(_, o)| ui.world.class(**o) == "GCUISimpleLabel").map(|(i, o)| (i, pos(*o))).collect();
            let images: Vec<(usize, Vec<f32>)> = objs.iter().enumerate().filter(|(_, o)| ui.world.class(**o) == "GCUISimpleImage").map(|(i, o)| (i, pos(*o))).collect();
            labels.sort_by(|a, b| (a.1[0], a.1[1]).partial_cmp(&(b.1[0], b.1[1])).unwrap());
            for (li, lp) in labels {
                let img = images.iter().filter(|(_, ip)| ip[0] < lp[0] && (ip[1] - lp[1]).abs() < 40.0).max_by(|a, b| a.1[0].partial_cmp(&b.1[0]).unwrap()).map(|x| x.0);
                ability_slots.push((li, img));
            }
        }
        let mut special_cost = HashMap::new();
        for s in ini("gcbattlepoints.default.specialabilitycosts") {
            if let (Some(c), Some(v)) = (ue_field(&s, "AbilityClassName"), ue_field(&s, "Cost")) {
                // one entry lacks the comma before PurchaseAmount ("Cost=1600 PurchaseAmount=1")
                let v: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(v) = v.parse() {
                    special_cost.insert(c.trim_matches('"').to_ascii_lowercase(), v);
                }
            }
        }
        let battle_points = ini("gcbattlepoints.default.initialbattlepoints").first().and_then(|v| v.parse().ok()).unwrap_or(0);
        // vehicles of the player's team: GCV_US_* for USA, GCV_CH_* for the KVA (the same
        // costs, GCBattlePoints VehicleCosts)
        let team = crate::combat::player_team();
        let prefix = if team == crate::combat::TEAM_USA { "GCV_US_" } else { "GCV_CH_" };
        let fmt = loc.get("UI_InGameMenus.SpawnSelection.VehicleCostInListBP").unwrap_or("%d - KP").to_string();
        let mut vehicles = vec![
            VehicleEntry { label: loc.get("ui_frontend.GCUIScene_SpawnSelection.m_InfantrySpawn").unwrap_or("").to_string(), cost: None, class: None, enabled: true },
            VehicleEntry { label: loc.get("ui_frontend.GCUIScene_SpawnSelection.m_TeamVehicle").unwrap_or("").to_string(), cost: None, class: None, enabled: false },
        ];
        let mut list: Vec<(i32, String)> = Vec::new();
        for v in ini("gcbattlepoints.default.vehiclecosts") {
            if let (Some(c), Some(cost)) = (ue_field(&v, "VehicleClassName"), ue_field(&v, "BPCost")) {
                let c = c.trim_matches('"');
                if c.starts_with(prefix) {
                    if let Ok(cost) = cost.parse::<i32>() {
                        list.push((cost, c.to_string()));
                    }
                }
            }
        }
        list.sort();
        for (cost, class) in list {
            let name = loc.get(&format!("gcgamecontent.{class}.VehicleNameString")).unwrap_or(&class).to_string();
            vehicles.push(VehicleEntry { label: format!("{} {name}", fmt.replace("%d", &cost.to_string())), cost: Some(cost), class: Some(class), enabled: cost <= battle_points });
        }
        let auto = ini("gcgame.gcgame.playerspawnmanagermp.spawnmenuautospawntime").first().and_then(|v| v.trim_end_matches(['f', ';']).parse::<f64>().ok()).unwrap_or(30.0);
        let tags = scene.widgets.iter().enumerate().map(|(i, w)| (w.tag.clone(), i)).collect();
        SpawnSel { loadouts, vehicles, sel_loadout: 0, sel_vehicle: 0, hover: None, auto, elapsed: 0.0, battle_points, alive: false, team, item_names, item_kind, item_image, ability_names, ability_icon, ability_slots, special_cost, tags }
    }

    /// Loadout labels of the scene (wMenuLabel0..).
    fn labels(&self) -> usize {
        (0..).take_while(|i| self.tags.contains_key(&format!("wMenuLabel{i}"))).count()
    }

    fn item(&self, c: &str) -> String {
        self.item_names.get(&c.to_ascii_lowercase()).cloned().unwrap_or_default()
    }

    /// Seconds left before the automatic spawn.
    pub fn remaining(&self, _now: f64) -> f64 {
        (self.auto - self.elapsed).max(0.0)
    }

    /// Advance the countdown by one frame (at most 0.1 s per frame).
    pub fn tick(&mut self, dt: f64) {
        self.elapsed += dt.clamp(0.0, 0.1);
    }

    /// Clicks are taken once the menu has been on screen for a moment (a click queued while
    /// the window was blocked must not hit "Spawn").
    pub fn accepts_input(&self) -> bool {
        self.elapsed >= 0.3
    }

    pub fn runtime(&self, ui: &Ui, rt: &mut HashMap<usize, WidgetRt>, colors: ([f32; 4], [f32; 4], [f32; 4]), now: f64) {
        let (unsel, sel, locked) = colors;
        let tags = &self.tags;
        macro_rules! set {
            ($tag:expr, $text:expr, $color:expr, $hidden:expr) => {
                set_rt(rt, tags, $tag, $text, $color, $hidden)
            };
        }
        for i in 0..self.labels() {
            let tag = format!("wMenuLabel{i}");
            match self.loadouts.get(i) {
                Some(l) => {
                    let c = if i == self.sel_loadout || self.hover == Some((0, i)) { sel } else { unsel };
                    set!(&tag, Some(l.name.clone()), Some(c), Some(false));
                }
                None => set!(&tag, None, None, Some(true)),
            }
        }
        for i in 0..8 {
            let tag = format!("MenuLabel{i}");
            match self.vehicles.get(i) {
                Some(v) => {
                    let c = if !v.enabled { locked } else if i == self.sel_vehicle || self.hover == Some((1, i)) { sel } else { unsel };
                    set!(&tag, Some(v.label.clone()), Some(c), Some(false));
                }
                None => set!(&tag, None, None, Some(true)),
            }
        }
        // the player's team flag over the lists (USA; trace t3 frame 1500 / user's video), the
        // KVA's (InfantryLoadoutPlacer "Korea" GCUIObject_TeamPlacer) after a team switch
        let usa = self.team == crate::combat::TEAM_USA;
        set!("USA", None, None, Some(!usa));
        set!("Korea", None, None, Some(usa));
        set!("USAFlag", None, None, Some(!usa));
        set!("NKorFlag", None, None, Some(usa));
        // bottom duplicate of the countdown and the weapon camo strip are not shown in the game
        set!("AutoTransitionLabel", None, None, Some(true));
        set!("WeaponCamoPattern", None, None, Some(true));
        if let Some(l) = self.loadouts.get(self.sel_loadout) {
            let img = |c: Option<&String>| c.and_then(|c| self.item_image.get(&c.to_ascii_lowercase()).copied());
            macro_rules! set_img {
                ($tag:expr, $g:expr) => {
                    if let (Some(&i), Some(g)) = (tags.get($tag), $g) {
                        rt.entry(i).or_default().image = Some(g);
                    }
                };
            }
            let kind_of = |k: &str| l.items.iter().filter(|c| self.item_kind.get(&c.to_ascii_lowercase()).is_some_and(|x| x == k)).cloned().collect::<Vec<_>>();
            set_img!("PrimaryWeaponName", img(l.items.first()));
            set_img!("WeaponAttachmentName", img(l.attachments.first()));
            set_img!("SpecialExplosiveName", img(kind_of("EIS_GrenadeSpecial").first()));
            let sp = kind_of("EIS_Special");
            set_img!("SpecialWeaponName", img(sp.first()));
            set_img!("InfantryAbilityName", img(sp.get(1)));
            if let Some(&w) = self.tags.get("EquipedAbilities") {
                let e = rt.entry(w).or_default();
                for (k, &(li, ii)) in self.ability_slots.iter().enumerate() {
                    let a = l.abilities.get(k).map(|a| a.to_ascii_lowercase());
                    let name = a.as_ref().and_then(|a| self.ability_names.get(a)).cloned();
                    e.simple.insert(li, upk::ui::SimpleRt { text: name.clone(), hidden: Some(name.is_none()), ..Default::default() });
                    if let Some(ii) = ii {
                        let icon = a.as_ref().and_then(|a| self.ability_icon.get(a)).copied();
                        e.simple.insert(ii, upk::ui::SimpleRt { image: icon, hidden: Some(name.is_none()), ..Default::default() });
                    }
                }
            }
            let primary = l.items.first().map(|c| self.item(c)).unwrap_or_default();
            set!("PrimaryWeaponName", Some(primary), None, None);
            set!("WeaponAttachmentName", Some(l.attachments.first().map(|c| self.item(c)).unwrap_or_default()), None, None);
            let kind = |k: &str| l.items.iter().filter(|c| self.item_kind.get(&c.to_ascii_lowercase()).is_some_and(|x| x == k)).cloned().collect::<Vec<_>>();
            let gs = kind("EIS_GrenadeSpecial");
            set!("SpecialExplosiveName", Some(gs.first().map(|c| self.item(c)).unwrap_or_default()), None, None);
            let sp = kind("EIS_Special");
            set!("SpecialWeaponName", Some(sp.first().map(|c| self.item(c)).unwrap_or_default()), None, None);
            set!("InfantryAbilityName", Some(sp.get(1).map(|c| self.item(c)).unwrap_or_default()), None, None);
            let bp = ui.loc.get("UI_InGameMenus.SpawnSelection.VehicleCostBP").unwrap_or("%d KP").to_string();
            let cost = |c: Option<&String>| c.and_then(|c| self.special_cost.get(&c.to_ascii_lowercase())).map(|v| bp.replace("%d", &v.to_string())).unwrap_or_default();
            set!("SpecialWeaponCost1", Some(cost(sp.first())), None, None);
            set!("SpecialWeaponCost2", Some(cost(sp.get(1))), None, None);
        }
        set!("CurrentBattlePoint", Some(format!("{}{}", self.battle_points, ui.loc.get("UI_HUD.InfantryHUD.BattlePoints").unwrap_or("KP"))), None, None);
        let left = self.remaining(now).ceil() as i32;
        set!("Timer", None, None, Some(self.alive));
        set!("Time", Some(format!("{}:{:02}", left / 60, left % 60)), None, Some(self.alive));
        set!("TransitionLabel", Some(ui.loc.get("UI_InGameMenus.SpawnSelection.AutoSpawning").unwrap_or("").to_string()), None, Some(self.alive));
    }

    /// Battle points after a death (vehicle entries enabled by cost) and the last loadout.
    pub fn resume(&mut self, kp: i32, loadout: usize, vehicle: usize) {
        self.battle_points = kp;
        for v in self.vehicles.iter_mut() {
            if let Some(c) = v.cost {
                v.enabled = c <= kp;
            }
        }
        if loadout < self.loadouts.len() {
            self.sel_loadout = loadout;
        }
        // the vehicle chosen before (pause menu), while it is still affordable
        self.sel_vehicle = if self.vehicles.get(vehicle).is_some_and(|v| v.enabled) { vehicle } else { 0 };
        // HB_SPAWNVEH=<index>: that entry when affordable (tests)
        if let Some(i) = std::env::var("HB_SPAWNVEH").ok().and_then(|v| v.parse::<usize>().ok()).filter(|&i| self.vehicles.get(i).is_some_and(|v| v.enabled)) {
            self.sel_vehicle = i;
        }
    }

    /// Ability names of the selected loadout (EquipedAbilities widget).
    pub fn abilities(&self) -> Vec<String> {
        self.loadouts.get(self.sel_loadout).map(|l| l.abilities.iter().map(|a| self.ability_names.get(&a.to_ascii_lowercase()).cloned().unwrap_or_default()).collect()).unwrap_or_default()
    }

    /// Mouse over / click on the two lists; returns true when consumed.
    pub fn mouse(&mut self, scene: &Scene, c: bevy::math::Vec2, pressed: bool) -> bool {
        self.hover = None;
        let inside = |r: [f32; 4]| c.x >= r[0] && c.x < r[2] && c.y >= r[1] && c.y < r[3];
        for i in 0..self.loadouts.len().min(self.labels()) {
            if let Some(&w) = self.tags.get(&format!("wMenuLabel{i}")) {
                if inside(scene.widgets[w].rect) {
                    self.hover = Some((0, i));
                    if pressed {
                        self.sel_loadout = i;
                        return true;
                    }
                }
            }
        }
        for i in 0..self.vehicles.len().min(8) {
            if let Some(&w) = self.tags.get(&format!("MenuLabel{i}")) {
                if inside(scene.widgets[w].rect) && self.vehicles[i].enabled {
                    self.hover = Some((1, i));
                    if pressed {
                        self.sel_vehicle = i;
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn set_rt(rt: &mut HashMap<usize, WidgetRt>, tags: &HashMap<String, usize>, tag: &str, text: Option<String>, color: Option<[f32; 4]>, hidden: Option<bool>) {
    if let Some(&i) = tags.get(tag) {
        let e = rt.entry(i).or_default();
        if text.is_some() {
            e.text = text;
        }
        if color.is_some() {
            e.text_color = color;
        }
        if hidden.is_some() {
            e.hidden = hidden;
        }
    }
}
