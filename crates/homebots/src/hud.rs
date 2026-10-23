//! In-game HUD: PC_UI_PlayerHUD.UIScenes.PC_U_PlayerHudScreen (GCUIScene_PlayerHud).
//! The scene's GC widgets draw GCUISimpleLabel/Image objects; their values are set here the way
//! the game's script does (trace t3 frame 2700 decoded with the HUDFont_Green glyph table:
//! timer "18:27" at x 67.2, "USA"/"KVA", KP "200" right-aligned at 1294.5, ammo "0"(dim)+"30",
//! grenade counts at 1138.7 / 1181.4).
use std::collections::HashMap;
use upk::ui::{GRef, Scene, SimpleRt, Ui, WidgetRt};

/// Top of the newest kill-crawl line (1280x720 units), calibrated to the user's video.
const CRAWL_BOTTOM_LINE_Y: f32 = 125.0;

/// Widgets of the infantry HUD that are shown; the rest (vehicle/drone placers, weapon-specific
/// placers, warnings and messages without an event) stay hidden.
const HIDDEN: &[&str] = &[
    "VehiclePlacerPredator",
    "VehiclePlacerGroundAssault",
    "VehiclePlacerGroundAntiVehicle",
    "VehiclePlacerAirRecon",
    "VehiclePlacerAirAssault",
    "VehiclePlacerAllDrones",
    "VehiclePlacerLAV",
    "VehiclePlacerLittleBird",
    "VehiclePlacerJeep",
    "VehiclePlacerAttackHeli",
    "VehiclePlacerTank",
    "VehiclePlacerMissileDefeat",
    "VehiclePlacerCommon",
    // repair / missile lock notices (no repairs or lock-ons offline)
    "VehicleRepairing",
    "MissileWarning",
    "TeammateDeathIcons",
    "PlayerNames",
    "HitDetector",
    "OutOfBoundsWarning",
    "ReconDroneMarker",
    "DeathMessage",
    "KillCrawl",
    "VehicleEjectionNotifier",
    "BannerMessage",
    "BattleCommanderStatus",
    "PlayerAbilities",
    "SavingGame",
    "PlayerExposedByRecon",
    "FullMap",
    "ProjectileWarning",
    "RangeToTarget_AirburstGL",
    "WeaponPlacerM200",
    "WeaponPlacerJavelin",
    "GameplayHint",
    "TextCrawl",
    "MOTD",
    // underbarrel attachment / resupply slots: only with such an item (assault loadout has none)
    "SpecialWeaponAttachment",
    "SpecialWeaponResupply",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Timer,
    TeamUsa,
    TeamKor,
    ScoreUsa,
    ScoreKor,
    Kp,
    Ammo,
    Grenade(usize),
    /// VehiclePlacerPredator GCUIObject_AirstrikeCounter: Hellfire missiles left
    AirstrikeLeft,
    ObjectiveText,
    Progress(usize),
}

/// Values shown by the HUD.
#[derive(Clone, Debug, Default)]
pub struct HudValues {
    pub time_left: f32,
    pub score: [i32; 2],
    /// ScoreLimitSlider of the match
    pub score_limit: i32,
    pub kp: i32,
    pub ammo: i32,
    pub grenades: [i32; 2],
    /// rounds in the magazine (bullet bar) and magazine size
    pub clip: i32,
    pub clip_max: i32,
    pub health: f32,
    /// objective markers (objectives.rs): position in 1280x720 units, letter, owner team,
    /// distance in metres, icon size
    pub markers: Vec<([f32; 2], usize, Option<u8>, f32, f32)>,
    /// rounds won (USA, KVA): GameScore score pills
    pub rounds: [i32; 2],
    /// banner text (GCUIBannerMessageManager / GCUIScene_WinLossSplash)
    pub banner: Option<String>,
    /// kill crawl lines: killer, killer team, victim, victim team, weapon UIIcon path
    pub kills: Vec<(String, u8, String, u8, Option<String>)>,
    /// weapon BulletIcon path and BulletIconSize (combat.rs)
    pub bullet_icon: Option<(String, [f32; 2])>,
    /// objective status line (LocalizedObjectiveStatus index, objective letter)
    pub objective_msg: Option<(usize, usize)>,
    /// objective the player stands in: letter, owner team, capture progress 0..1
    pub active_obj: Option<(usize, Option<u8>, f32)>,
    /// minimap centre / heading and icons (minimap.rs)
    pub minimap: Option<crate::minimap::MinimapHud>,
    /// game time (s) for HUD animations
    pub clock: f32,
    /// KP as displayed (counts up to `kp`, objectives.rs)
    pub kp_shown: f32,
    /// UAV sweep seconds left (enemies on the minimap)
    pub uav_left: f32,
    /// rewards waiting for the banner: amount, GCPointSpendingSystem reward id (PSR_*)
    pub rewards: Vec<(i32, &'static str)>,
    /// GCUIBannerMessageManager message: line 1, line 2, start (clock), slow animation
    pub banner_msg: Option<(String, String, f32, bool)>,
    /// special weapon slots (keys 4 / 5): UIIcon_Small path, uses shown (None: no number),
    /// red (used up equipment such as the worn flak jacket), selected
    /// special weapon slots: (icon, uses, red, selected, affordable)
    pub specials: [(Option<String>, Option<i32>, bool, bool, bool); 2],
    /// hip crosshair: inner end of the dashes from the centre (1280x720 units); None hidden
    pub crosshair: Option<f32>,
    /// the same for a vehicle gun seat (vehicles.rs)
    pub vehicle_crosshair: Option<f32>,
    /// hit notify fade (seconds left of HitEnemyFadeTime)
    pub hit_marker: f32,
    /// GCUIObject_HitDetector locators (dmgfeedback.rs): direction of the hit clockwise from
    /// the view (radians), opacity
    pub hit_locators: Vec<(f32, f32)>,
    /// dead: (killer name, own fault), seconds until the spawn selection (kill cam overlay)
    pub death: Option<(Option<(String, bool)>, i32)>,
    /// seat in the HUMVEE (0 driver, 1 gunner): VehiclePlacerJeep / Common instead of the
    /// infantry placer
    pub vehicle_seat: Option<usize>,
    /// the vehicle's class chain (GCV_US_M1A3_Content, GCVTank, GCVehicle, ...): its placers by
    /// VehicleTypes and their Seat<n>Placer (vehicles.rs)
    pub vehicle_classes: Vec<String>,
    /// HUMVEE gauges: sprint left 0..1, sprinting, gun heat 0..1 (overheated)
    pub vehicle_gauges: Option<(f32, bool, f32, bool)>,
    /// vehicle view: first person (the gunsight MilitaryHUD shows only then)
    pub vehicle_first_person: bool,
    /// GCUIObject_RangeToTarget: distance to the aim point (m), None = nothing hit
    pub vehicle_range: Option<f32>,
    /// flash bang white-out seconds left
    pub blinded: f32,
    /// remote ground drone: VehiclePlacerAllDrones + VehiclePlacerGroundAssault, battery 0..1,
    /// overheat 0..1
    pub drone: Option<(f32, f32)>,
    /// drone gun lock-on box (GCUIObject_TargetingBox, LockOn.BigRectangle corners around the centre)
    pub lock_box: Option<([f32; 2], [f32; 2])>,
    /// the held launcher's lock-on (combat.rs player_lock_on): BigRectangle corners around the
    /// centre, and the target's box (SmallRectangle material corners around its screen point,
    /// 1280x720 units), lock progress 0..1, locked
    pub weapon_lock: Option<WeaponLockHud>,
    /// air recon drone markers: (1280x720 position, marked) - ReconDroneMarkers_Marked /
    /// _UnMarked
    pub recon: Vec<(bevy::math::Vec2, bool)>,
    /// the drone's class chain (GCV_SH_GAntiTankWalker_Content, GCV_SH_GAntiTank_Content): the
    /// placers whose VehicleTypes name one of them are shown
    pub drone_classes: Vec<String>,
    /// airstrike target marker (Targeter02 top faces): (centre, length, width, angle) quads in
    /// 1280x720 units
    pub strike_marker: Vec<(bevy::math::Vec2, f32, f32, f32)>,
    /// Hellfire UAV (airstrike.rs): VehiclePlacerPredator, fuel 0..1, missiles left
    pub predator: Option<(f32, i32)>,
    /// GCUIObject_GameplayHint line: "deploy_drone" = "(LMT) ZUM ENTSENDEN EINER DROHNE"
    pub hint: Option<&'static str>,
    /// C4 detonator in hand with a charge out: "(LMT) ZUM DETONIEREN VON C4"
    /// (UI_GameplayHints C4Detonate, user's screenshots of the original)
    pub c4_hint: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WeaponLockHud {
    pub big: ([f32; 2], [f32; 2]),
    pub target: Option<([f32; 2], [f32; 2], f32, bool)>,
}

/// PC_Textures.EnemyBlip "Color" default (UAV-revealed enemies on the minimap).
const ENEMY_BLIP: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

pub struct Hud {
    hidden: Vec<usize>,
    roles: Vec<(usize, usize, Role)>,
    /// GCUIObject_AmmoCounter: widget, LowerRightBulletStartPosition, CounterLength,
    /// BulletIconAlpha, EmptyBulletIconAlpha
    ammo: Option<(usize, [f32; 2], f32, f32, f32)>,
    /// GCUIObject_GameObjectives widget and the team label colours (USA, KVA)
    objectives: Option<usize>,
    team_colors: [[f32; 4]; 2],
    /// GCUIObject_KillCrawl: widget, top-left (1280x720), TextScale, IconScale
    crawl: Option<(usize, [f32; 2], f32, f32)>,
    /// GameScore FilledFriendlyScorePills / FilledEnemyScorePills simple-object indices
    pills: Option<(usize, Vec<usize>, Vec<usize>)>,
    minimap: Option<MinimapWidget>,
    /// GCUIObject_SpecialWeapon 1 / 2: widget, iWeaponIcon and iNumUsesLabel simple indices,
    /// AlphaOn / AlphaOff
    specials: [Option<(usize, usize, usize, f32, f32)>; 2],
    /// GCUIObject_MiniMap's MiniMapEffects placer (colour effect, scan lines)
    minimap_effects: Option<usize>,
    /// GCUIObject_WeaponCrosshair: widget, CrosshairTexture (vertical dash), CrosshairTexture_Left
    /// (horizontal), length / thickness multipliers, hit notify texture
    crosshair: Option<(usize, GRef, GRef, f32, f32, Option<GRef>)>,
    /// widgets hidden while the kill cam shows (all but the objectives widget and its parents)
    death_hide: Vec<usize>,
    /// HUD modes: infantry placer, vehicle / drone placers by tag
    infantry: Vec<usize>,
    placers: HashMap<String, usize>,
    /// (placer, vehicle class) from the placers' VehicleTypes
    placer_types: Vec<(String, String)>,
    /// seat placers ("<placer>.Seat<n>Placer") with their SeatIndex
    seat_index: HashMap<String, i32>,
    /// the MilitaryHUD sub-placers (gunsight overlays, first person only) and the
    /// GCUIObject_RangeToTarget widgets with their label simple objects
    military: Vec<usize>,
    range_labels: Vec<(usize, Vec<usize>)>,
    /// GCUIObject_Placer widgets under a vehicle seat placer (the seats' reticules): their
    /// GCUISimpleImage positions are absolute 1280x720 coordinates (VehiclePlacerTank
    /// Seat1Placer.Reticule: Position 0.305..0.695 x 0.361..0.639 of the viewport, the
    /// Reticule_Tank_Circle at 624,344 size 32 = the screen centre), not offsets from the
    /// widget's corner
    seat_reticules: Vec<usize>,
    /// ReconDroneMarker widget and the ReconDroneMarkers_Marked / _UnMarked textures
    recon_widget: Option<usize>,
    recon_tex: (Option<GRef>, Option<GRef>),
    /// GCUIObject_DroneBatteryLife: widget and its two images (frame / fill)
    battery: Option<(usize, Vec<usize>)>,
    /// the Predator placer's DroneBatteryLife (PredatorDrone.DroneBatteryLife)
    battery_pred: Option<(usize, Vec<usize>)>,
    /// VehiclePlacerCommon: VehicleSpeedometer (widget, SprintMeter, Message) and
    /// GCUIObject_WeaponReloadOverheat (widget, FireModeProgress[0])
    /// (the simple object lists hold an object twice: SimpleObjects + CodeBasedSimpleObjects)
    speedo: Option<(usize, Vec<usize>, Vec<usize>)>,
    overheat: Option<(usize, Vec<usize>)>,
    /// UI_Common.Textures.T_DefaultWhite (flash bang white-out)
    white: Option<GRef>,
    /// GCUIObject_VehicleOccupancy: widget, per seat (SeatFilled, Gamertag, VOIPIcon) simple
    /// indices, SeatLabel indices
    occupancy: Option<(usize, [(Vec<usize>, Vec<usize>, Vec<usize>); 2], Vec<usize>)>,
    /// GCUIObject_FullMap: widget, icons (PlayerIcon, Alpha..Delta, objective size,
    /// FriendlyInfantryIcon)
    full_map: Option<FullMapWidget>,
    /// HUD preferences (U_HUDPrefs, PlayerDetails markup) -> the widgets they hide
    prefs: Vec<(&'static str, usize)>,
    /// GCUIObject_HitDetector widget and its HitLocatorTexture (dmgfeedback.rs)
    hit_detector: Option<(usize, Option<GRef>)>,
}

/// PlayerDetails HUD flag -> PC_U_PlayerHudScreen widget tag (by the widgets' classes:
/// GCUIObject_HUDPlacer Minimap, AmmoCounter / GrenadeCounter, TextCrawl, GameScore,
/// SpecialWeapons placer, WeaponCrosshair, TotalBattlePoints, GameplayHint)
const PREF_WIDGETS: &[(&str, &str)] = &[
    ("PlayerDetails:Minimap", "Minimap_HUDPlacer"),
    ("PlayerDetails:AmmoMeter", "AmmoCounter"),
    ("PlayerDetails:AmmoMeter", "GrenadeCounter"),
    ("PlayerDetails:ChatText", "TextCrawl"),
    ("PlayerDetails:GameScore", "GameScore"),
    ("PlayerDetails:SpecialWeapons", "SpecialWeapons_HUDPlacer"),
    ("PlayerDetails:WeaponReticules", "WeaponCrosshair"),
    ("PlayerDetails:BattlePoints", "TotalBattlePoints"),
    ("PlayerDetails:GameHints", "GameplayHint"),
];

struct FullMapWidget {
    wi: usize,
    /// Backing / FullMapEffects (GreenOverlay) placers: their stored positions put a black square
    /// and a green rectangle beside the map (the code places them), hidden
    children: Vec<usize>,
    capture: Option<GRef>,
    player: Option<(GRef, f32)>,
    objectives: [Option<GRef>; 4],
    objective_size: f32,
    friend: Option<(GRef, f32)>,
    enemy: Option<(GRef, f32)>,
}

/// GCUIObject_MiniMap: MapTexture rect (1280x720) and the icon set.
struct MinimapWidget {
    wi: usize,
    rect: [f32; 4],
    player: Option<(GRef, f32)>,
    objectives: [Option<GRef>; 4],
    objective_size: f32,
    friend: Option<(GRef, f32)>,
    enemy: Option<(GRef, f32)>,
}

impl Hud {
    pub fn new(ui: &Ui, scene: &Scene) -> Hud {
        let mut hidden = Vec::new();
        let mut roles = Vec::new();
        let mut ammo = None;
        let mut objectives = None;
        let mut crawl = None;
        let mut pills = None;
        let mut minimap = None;
        let mut minimap_effects = None;
        let mut specials = [None, None];
        let mut crosshair = None;
        let mut full_map = None;
        let mut infantry = Vec::new();
        let mut placers = HashMap::new();
        let mut placer_types: Vec<(String, String)> = Vec::new();
        let mut seat_index: HashMap<String, i32> = HashMap::new();
        let mut military = Vec::new();
        let seat_reticules: Vec<usize> = scene.widgets.iter().enumerate().filter(|(_, w)| w.class == "GCUIObject_Placer" && w.parent.is_some_and(|p| scene.widgets[p].class == "GCUIObject_VehicleSeatPlacer")).map(|(i, _)| i).collect();
        let mut range_labels = Vec::new();
        let mut battery = None;
        let mut battery_pred = None;
        let mut speedo = None;
        let mut overheat = None;
        let mut occupancy = None;
        let mut team_colors = [[0.026549, 0.420508, 0.796917, 1.0], [0.8, 0.1, 0.1, 1.0]];
        for (wi, w) in scene.widgets.iter().enumerate() {
            if w.class == "GCUIObject_GameObjectives" {
                objectives = Some(wi);
            }
            if w.class == "GCUIObject_GameScore" {
                let objs = ui.simple_objects(&w.props);
                let idx = |name: &str| -> Vec<usize> { ui.world.objects(w.props.get(name)).iter().filter_map(|o| objs.iter().position(|x| x == o)).collect() };
                pills = Some((wi, idx("FilledFriendlyScorePills"), idx("FilledEnemyScorePills")));
            }
            if w.class == "GCUIObject_KillCrawl" {
                // Position (PercentageViewport) 0.018823802, 0.03597302 of 1280x720
                // x: Position 0.018823802 of 1280; the newest line's top from the user's video
                // (capture message, cap height at y 131..141) - the box (top 0.036, height 0.205)
                // ends at y 174, the lines' vertical placement is code
                crawl = Some((wi, [0.018823802 * 1280.0, CRAWL_BOTTOM_LINE_Y], w.props.floats("TextScale", 2).map(|v| v[0]).unwrap_or(0.6), w.props.f32("IconScale").unwrap_or(0.6)));
            }
            if w.class == "GCUIObject_SpecialWeapon" && (w.tag == "SpecialWeapon1" || w.tag == "SpecialWeapon2") {
                let objs = ui.simple_objects(&w.props);
                let idx = |k: &str| w.props.obj(k).and_then(|o| objs.iter().position(|x| *x == o));
                if let (Some(icon), Some(label)) = (idx("WeaponIcon"), idx("NumUsesLabel")) {
                    specials[if w.tag == "SpecialWeapon1" { 0 } else { 1 }] = Some((wi, icon, label, w.props.f32("AlphaOn").unwrap_or(1.0), w.props.f32("AlphaOff").unwrap_or(0.8)));
                }
            }
            if w.class == "GCUIObject_WeaponCrosshair" && crosshair.is_none() {
                if let (Some(v), Some(h)) = (w.props.obj("CrosshairTexture"), w.props.obj("CrosshairTexture_Left")) {
                    // M_Crosshair_HitNotify samples TX_Reticule_Hit_v2
                    let hit = ui.world.find("UI_PlayerHUD.Textures.TX_Reticule_Hit_v2");
                    crosshair = Some((wi, v, h, w.props.f32("CrosshairImageSizeLengthMultiplier").unwrap_or(1.0), w.props.f32("CrosshairImageSizeThicknessMultiplier").unwrap_or(1.0), hit));
                }
            }
            if w.class == "GCUIObject_InfantryPlacer" {
                infantry.push(wi);
            }
            if w.class == "GCUIObject_VehiclePlacer" || w.class == "GCUIObject_VehicleSeatPlacer" {
                // seat placers by parent: "VehiclePlacerJeep.Seat1Placer"
                let key = match w.parent.map(|p| scene.widgets[p].tag.clone()) {
                    Some(pt) if w.class == "GCUIObject_VehicleSeatPlacer" => format!("{pt}.{}", w.tag),
                    _ => w.tag.clone(),
                };
                // VehicleTypes (EVT_A<class>): the vehicle classes the placer is for
                for t in ui.world.name_list(w.props.get("VehicleTypes")) {
                    if let Some(c) = t.strip_prefix("EVT_A") {
                        placer_types.push((key.clone(), c.to_string()));
                    }
                }
                if w.class == "GCUIObject_VehicleSeatPlacer" {
                    // SeatIndex (missing = 0); a value not readable as an int falls back to the tag's number
                    let from_tag = w.tag.trim_start_matches("Seat").trim_end_matches("Placer").parse::<i32>().map(|n| n - 1).unwrap_or(0);
                    seat_index.insert(key.clone(), w.props.i32("SeatIndex").unwrap_or(if w.props.get("SeatIndex").is_some() { from_tag } else { 0 }));
                }
                if w.tag == "MilitaryHUD" {
                    military.push(wi);
                }
                placers.entry(key).or_insert(wi);
            }
            if w.class == "GCUIObject_RangeToTarget" {
                let objs = ui.simple_objects(&w.props);
                range_labels.push((wi, (0..objs.len()).collect()));
            }
            if w.class == "GCUIObject_DroneBatteryLife" && w.parent.is_some_and(|p| scene.widgets[p].tag == "PredatorDrone") {
                let objs = ui.simple_objects(&w.props);
                let bar = w.props.obj("BatteryLife_S").and_then(|o| objs.iter().position(|x| *x == o));
                battery_pred = Some((wi, bar.into_iter().collect()));
            }
            if w.class == "GCUIObject_DroneBatteryLife" && w.parent.is_some_and(|p| scene.widgets[p].tag == "VehiclePlacerAllDrones") {
                let objs = ui.simple_objects(&w.props);
                let bar = w.props.obj("BatteryLife_S").and_then(|o| objs.iter().position(|x| *x == o));
                battery = Some((wi, bar.into_iter().collect()));
            }
            let in_common = w.parent.is_some_and(|p| scene.widgets[p].tag == "VehiclePlacerCommon");
            if in_common && w.class == "GCUIObject_VehicleSpeedometer" {
                let objs = ui.simple_objects(&w.props);
                let idx = |k: &str| -> Vec<usize> { w.props.obj(k).map(|o| objs.iter().enumerate().filter(|(_, x)| **x == o).map(|(i, _)| i).collect()).unwrap_or_default() };
                speedo = Some((wi, idx("SprintMeter"), idx("Message")));
            }
            if in_common && w.class == "GCUIObject_VehicleOccupancy" {
                let objs = ui.simple_objects(&w.props);
                let at = |o: GRef| -> Vec<usize> { objs.iter().enumerate().filter(|(_, x)| **x == o).map(|(i, _)| i).collect() };
                let list = |n: &str| ui.world.objects(w.props.get(n));
                let (filled, tags, voip) = (list("SeatFilled"), list("Gamertag"), list("VOIPIcon"));
                let seat = |k: usize| (filled.get(k).map(|o| at(*o)).unwrap_or_default(), tags.get(k).map(|o| at(*o)).unwrap_or_default(), voip.get(k).map(|o| at(*o)).unwrap_or_default());
                let label = list("SeatLabel").first().map(|o| at(*o)).unwrap_or_default();
                occupancy = Some((wi, [seat(0), seat(1)], label));
            }
            if in_common && w.class == "GCUIObject_WeaponReloadOverheat" {
                let objs = ui.simple_objects(&w.props);
                let bar: Vec<usize> = ui.world.objects(w.props.get("FireModeProgress")).first().map(|o| objs.iter().enumerate().filter(|(_, x)| *x == o).map(|(i, _)| i).collect()).unwrap_or_default();
                overheat = Some((wi, bar));
            }
            if w.class == "GCUIObject_FullMap" && full_map.is_none() {
                let icon = |name: &str| w.props.sub(name).and_then(|s| Some((s.obj("Texture")?, s.f32("Size").unwrap_or(16.0))));
                let oi = w.props.sub("ObjectiveIcon");
                let letter = |n: &str| oi.as_ref().and_then(|s| s.obj(n));
                full_map = Some(FullMapWidget {
                    wi,
                    children: w.children.clone(),
                    // M_FullMapTerrain: emissive = Textures.MinimapRenderTexture (the capture)
                    capture: ui.world.find("UI_PlayerHUD.Textures.MinimapRenderTexture"),
                    player: icon("PlayerIcon"),
                    objectives: [letter("Alpha"), letter("Bravo"), letter("Charlie"), letter("Delta")],
                    objective_size: oi.as_ref().and_then(|s| s.f32("Size")).unwrap_or(24.0),
                    friend: icon("FriendlyInfantryIcon"),
                    enemy: icon("EnemyInfantryIcon"),
                });
            }
            if w.tag == "MiniMapEffects" {
                minimap_effects = Some(wi);
            }
            if w.class == "GCUIObject_MiniMap" {
                // MapTexture (GCUISimpleImage_0, M_MiniMapTerrain) Position / Size, icon structs
                if let Some(m) = w.props.obj("MapTexture") {
                    let mp = ui.world.props(m);
                    let pos = mp.floats("Position", 2).unwrap_or(vec![0.0, 0.0]);
                    let size = mp.floats("Size", 2).unwrap_or(vec![64.0, 64.0]);
                    let icon = |name: &str| w.props.sub(name).and_then(|s| Some((s.obj("Texture")?, s.f32("Size").unwrap_or(16.0))));
                    let oi = w.props.sub("ObjectiveIcon");
                    let letter = |n: &str| oi.as_ref().and_then(|s| s.obj(n));
                    minimap = Some(MinimapWidget {
                        wi,
                        rect: [pos[0], pos[1], pos[0] + size[0], pos[1] + size[1]],
                        player: icon("PlayerIcon"),
                        objectives: [letter("Alpha"), letter("Bravo"), letter("Charlie"), letter("Delta")],
                        objective_size: oi.as_ref().and_then(|s| s.f32("Size")).unwrap_or(24.0),
                        friend: icon("FriendlyInfantryIcon"),
                        enemy: icon("EnemyInfantryIcon"),
                    });
                }
            }
            if w.class == "GCUIObject_AmmoCounter" {
                let st = w.props.floats("LowerRightBulletStartPosition", 2).unwrap_or(vec![0.0, 0.0]);
                ammo = Some((wi, [st[0], st[1]], w.props.f32("CounterLength").unwrap_or(117.0), w.props.f32("BulletIconAlpha").unwrap_or(1.0), w.props.f32("EmptyBulletIconAlpha").unwrap_or(0.2)));
            }
            if HIDDEN.contains(&w.tag.as_str()) {
                hidden.push(wi);
            }
            let objs = ui.simple_objects(&w.props);
            let mut grenade = 0;
            for (si, &o) in objs.iter().enumerate() {
                let op = ui.world.props(o);
                let class = ui.world.class(o);
                let text = op.str("Text").unwrap_or("");
                let role = match (w.tag.as_str(), class.as_str(), text) {
                    ("GameScore", _, "88:88") => Some(Role::Timer),
                    ("GameScore", _, "USA") => {
                        if let Some(c) = op.lcolor("DrawColor") {
                            team_colors[0] = c;
                        }
                        Some(Role::TeamUsa)
                    }
                    ("GameScore", _, "KOR") => {
                        if let Some(c) = op.lcolor("DrawColor") {
                            team_colors[1] = c;
                        }
                        Some(Role::TeamKor)
                    }
                    ("GameScore", _, "99999") => {
                        // the first of each pair is USA (y 975), the second KOR (y 990)
                        let y = op.floats("Position", 2).map(|p| p[1]).unwrap_or(0.0);
                        Some(if y < 980.0 { Role::ScoreUsa } else { Role::ScoreKor })
                    }
                    ("TotalBattlePoints", "GCUISimpleLabel", "0") => Some(Role::Kp),
                    ("AmmoCounter", "GCUISimpleLabel_Integer", _) => Some(Role::Ammo),
                    ("GameObjectivesMP", "GCUISimpleLabel", _) => Some(Role::ObjectiveText),
                    // FriendlyProgress (USA) / EnemyProgress: GCUISimpleProgressBar_1 / _2
                    ("GameScore", "GCUISimpleProgressBar", _) => Some(Role::Progress(if ui.world.path(o).ends_with("_1") { 0 } else { 1 })),
                    ("AirstrikeCounter", "GCUISimpleLabel", _) => Some(Role::AirstrikeLeft),
                    ("GrenadeCounter", "GCUISimpleLabel", "2") => {
                        // pairs: x 1107 / 1067 -> the left one (1067) is index 0
                        let x = op.floats("Position", 2).map(|p| p[0]).unwrap_or(0.0);
                        let _ = grenade;
                        grenade += 1;
                        Some(Role::Grenade(if x < 1090.0 { 0 } else { 1 }))
                    }
                    _ => None,
                };
                if let Some(r) = role {
                    roles.push((wi, si, r));
                }
            }
        }
        let mut keep = Vec::new();
        let mut cur = objectives;
        while let Some(k) = cur {
            keep.push(k);
            cur = scene.widgets[k].parent;
        }
        let death_hide = (0..scene.widgets.len()).filter(|k| !keep.contains(k)).collect();
        let prefs = PREF_WIDGETS.iter().filter_map(|(k, tag)| scene.widgets.iter().position(|w| w.tag == *tag).map(|i| (*k, i))).collect();
        bevy::log::info!("hud: vehicle placers {:?}", placer_types);
        Hud { hidden, roles, ammo, objectives, team_colors, crawl, pills, minimap, minimap_effects, specials, crosshair, death_hide, full_map, infantry, placers, placer_types, seat_index, military, range_labels, seat_reticules, recon_widget: scene.widgets.iter().position(|w| w.tag == "ReconDroneMarker"), recon_tex: (ui.world.find("UI_PlayerHUD.Textures.ReconDroneMarkers_Marked"), ui.world.find("UI_PlayerHUD.Textures.ReconDroneMarkers_UnMarked")), battery, battery_pred, speedo, overheat, white: ui.world.find("UI_Common.Textures.T_DefaultWhite"), occupancy, prefs, hit_detector: scene.widgets.iter().position(|w| w.class == "GCUIObject_HitDetector").map(|wi| (wi, ui.world.objects(scene.widgets[wi].props.get("HitLocatorTexture")).first().copied())) }
    }

    pub fn runtime(&self, ui: &Ui, v: &HudValues) -> HashMap<usize, WidgetRt> {
        let mut rt: HashMap<usize, WidgetRt> = HashMap::new();
        for &w in &self.hidden {
            rt.entry(w).or_default().hidden = Some(true);
        }
        // damage direction (GCUIObject_HitDetector, native drawing not decoded): HitLocatorTexture
        // (UI_PlayerHUD Hit_Locator, an arc) at Default__GCUIObject_HitDetector HitDetectorSize
        // 256 x 64, HitDetectorRadius 256 from the screen centre towards the hit (up = ahead),
        // turned with it, fading with dmgfeedback.rs. NOT from data: centre (640, 360), the arc's
        // middle on the radius, linear fade.
        if let Some((wi, Some(tex))) = self.hit_detector {
            if !v.hit_locators.is_empty() && v.death.is_none() {
                let r = rt.entry(wi).or_default();
                r.hidden = Some(false);
                for &(a, alpha) in &v.hit_locators {
                    let (cx, cy) = (640.0 + 256.0 * a.sin(), 360.0 - 256.0 * a.cos());
                    r.extra_rot.push((tex, [cx - 128.0, cy - 32.0, cx + 128.0, cy + 32.0], [1.0, 1.0, 1.0, alpha], a));
                }
            }
        }
        // magazine bullets (GCUIObject_AmmoCounter): BulletIconSize icons spread over CounterLength
        // from LowerRightBulletStartPosition leftwards, spent rounds at EmptyBulletIconAlpha
        if let (Some((wi, start, len, full_a, empty_a)), Some((icon, size))) = (self.ammo, v.bullet_icon.as_ref()) {
            if let Some(tex) = ui.world.find(icon) {
                let n = v.clip_max.max(1);
                let step = len / n as f32;
                let color = [0.505433, 1.0, 0.612066, 1.0];
                let r = rt.entry(wi).or_default();
                for k in 0..n {
                    let x1 = start[0] - k as f32 * step;
                    let alive = k < v.clip;
                    let mut c = color;
                    c[3] = if alive { full_a } else { empty_a };
                    r.extra.push((tex, [x1 - size[0], start[1] - size[1], x1, start[1]], c));
                }
            }
        }
        // objective markers: UI_PlayerHUD Obj_Alpha/Bravo/Charlie (m_ObjectiveIcon Texture parameter)
        // tinted by owner, distance in DistanceLabelFont at DistanceLabelFontScale 0.6, alpha 0.5
        if let Some(wi) = self.objectives {
            let r = rt.entry(wi).or_default();
            let font = ui.world.find("UI_Fonts.HUD.GamerTagFont");
            let markers: &[_] = if crate::settings::flag("PlayerDetails:ObjectiveIndicators") { &v.markers } else { &[] };
            for &(pos, letter, owner, dist, size) in markers {
                let name = ["UI_PlayerHUD.Textures.Obj_Alpha", "UI_PlayerHUD.Textures.Obj_Bravo", "UI_PlayerHUD.Textures.Obj_Charlie", "UI_PlayerHUD.Textures.Obj_Delta"][letter.min(3)];
                let Some(tex) = ui.world.find(name) else { continue };
                let color = match owner {
                    Some(t) => self.team_colors[rel_team(t)],
                    None => [1.0, 1.0, 1.0, 1.0],
                };
                let h = size * 0.5;
                r.extra.push((tex, [pos[0] - h, pos[1] - h, pos[0] + h, pos[1] + h], color));
                r.extra_text.push((format!("{} M", dist.round() as i32), font, [pos[0], pos[1] + h + 1.0], 0.6, [1.0, 1.0, 1.0, 0.5], true));
            }
            // active objective: ActiveObjectiveCenterPosition (640, 132), ActiveObjectiveSize 64,
            // ProgressIconTemplate M_roundedMask while it is being taken (user's video 56-75 s:
            // white letter + ring, then the letter in the team colour without ring)
            if let Some((letter, owner, progress)) = v.active_obj {
                let name = ["UI_PlayerHUD.Textures.Obj_Alpha", "UI_PlayerHUD.Textures.Obj_Bravo", "UI_PlayerHUD.Textures.Obj_Charlie", "UI_PlayerHUD.Textures.Obj_Delta"][letter.min(3)];
                let rect = [640.0 - 32.0, 132.0 - 32.0, 640.0 + 32.0, 132.0 + 32.0];
                if let Some(tex) = ui.world.find(name) {
                    let color = match owner {
                        Some(t) => self.team_colors[rel_team(t)],
                        None => [1.0, 1.0, 1.0, 1.0],
                    };
                    r.extra.push((tex, rect, color));
                }
                if progress > 0.0 {
                    if let Some(ring) = ui.world.find("UI_PlayerHUD.Materials.M_roundedMask") {
                        r.extra.push((ring, rect, [1.0; 4]));
                    }
                }
            }
        }
        // kill crawl (GCUIObject_KillCrawl TextFont GamerTagFont, TextScale 0.6, weapon UIIcon at
        // IconScale 0.6): killer [icon] victim, newest at the bottom; names in team colours
        if let Some((wi, origin, ts, is)) = self.crawl {
            let r = rt.entry(wi).or_default();
            r.hidden = Some(false);
            let font = ui.world.find("UI_Fonts.HUD.GamerTagFont");
            let line = 22.0;
            let col = |t: u8| if t > 1 { [1.0, 1.0, 1.0, 1.0] } else { self.team_colors[rel_team(t)] };
            let n = v.kills.len();
            for (k, (killer, kt, victim, vt, icon)) in v.kills.iter().enumerate() {
                // newest line at the bottom slot (user's video: a single crawl line at y 131)
                let y = origin[1] - (n - 1 - k) as f32 * line;
                let mut x = origin[0];
                r.extra_text.push((killer.clone(), font, [x, y], ts, col(*kt), false));
                if victim.is_empty() {
                    continue;
                }
                x += ui.text_width_1280(killer, font, ts) + 8.0;
                if let Some(tex) = icon.as_deref().and_then(|p| ui.world.find(p)) {
                    let (w, h) = (128.0 * is * 0.5, 32.0 * is * 0.5);
                    r.extra.push((tex, [x, y, x + w, y + h], [1.0, 1.0, 1.0, 1.0]));
                    x += w + 8.0;
                } else {
                    r.extra_text.push(("+".into(), font, [x, y], ts, [1.0, 1.0, 1.0, 1.0], false));
                    x += 16.0;
                }
                r.extra_text.push((victim.clone(), font, [x, y], ts, col(*vt), false));
            }
        }
        // minimap icons (GCUIObject_MiniMap): world -> map with the capture's scale (MapTexture
        // width = minimap::VIEW_UU), heading up; objectives clamped to the square's edge
        // (SquareMinimap; the user's video shows "A" on the border), player arrow in the centre
        if let (Some(mw), Some(mm)) = (&self.minimap, &v.minimap) {
            let r = rt.entry(mw.wi).or_default();
            let (cx, cy) = ((mw.rect[0] + mw.rect[2]) * 0.5, (mw.rect[1] + mw.rect[3]) * 0.5);
            let (hw, hh) = ((mw.rect[2] - mw.rect[0]) * 0.5, (mw.rect[3] - mw.rect[1]) * 0.5);
            let s = (mw.rect[2] - mw.rect[0]) / crate::minimap::VIEW_UU;
            let (sy, cyaw) = mm.yaw.sin_cos();
            let to_map = |p: [f32; 2]| {
                let (dx, dy) = (p[0] - mm.center[0], p[1] - mm.center[1]);
                // UE: forward (cos, sin), right (-sin, cos)
                let fwd = dx * cyaw + dy * sy;
                let right = -dx * sy + dy * cyaw;
                (right * s, -fwd * s)
            };
            if let Some((tex, size)) = mw.friend {
                for &f in &mm.friends {
                    let (x, y) = to_map(f);
                    if x.abs() > hw || y.abs() > hh {
                        continue;
                    }
                    let h = size * 0.5;
                    r.extra.push((tex, [cx + x - h, cy + y - h, cx + x + h, cy + y + h], [1.0; 4]));
                }
            }
            if let Some((tex, size)) = mw.enemy {
                for &f in &mm.enemies {
                    let (x, y) = to_map(f);
                    if x.abs() > hw || y.abs() > hh {
                        continue;
                    }
                    let h = size * 0.5;
                    // enemies shown by the UAV in red: GCUIObject_MiniMap EnemyBlipMaterial
                    // PC_Textures.EnemyBlip, VectorParameter "Color" (1, 0, 0, 1); team mates stay
                    // white (FriendlyInfantryIcon untinted)
                    r.extra.push((tex, [cx + x - h, cy + y - h, cx + x + h, cy + y + h], ENEMY_BLIP));
                }
            }
            for &(p, letter, owner) in &mm.objectives {
                let Some(tex) = mw.objectives[letter.min(3)] else { continue };
                let (mut x, mut y) = to_map(p);
                let h = mw.objective_size * 0.5;
                let k = (x.abs() / (hw - h).max(1.0)).max(y.abs() / (hh - h).max(1.0));
                if k > 1.0 {
                    x /= k;
                    y /= k;
                }
                let color = match owner {
                    Some(t) => self.team_colors[rel_team(t)],
                    None => [1.0; 4],
                };
                r.extra.push((tex, [cx + x - h, cy + y - h, cx + x + h, cy + y + h], color));
            }
            if let Some((tex, size)) = mw.player {
                let h = size * 0.5;
                r.extra.push((tex, [cx - h, cy - h, cx + h, cy + h], [1.0; 4]));
            }
        }
        // special weapon slots: the loadout's 4th / 5th item icon (UIIcon_Small), uses label;
        // user's video 80-84 s: flak jacket icon turns red with "1" once bought, the Panzerfaust
        // shows its rounds ("3"); nothing bought -> no number. Selected slot at AlphaOn, the
        // other at AlphaOff (GCUIObject_SpecialWeapon).
        for (k, sw) in self.specials.iter().enumerate() {
            let Some((wi, si_icon, si_label, a_on, a_off)) = *sw else { continue };
            let (icon, uses, red, selected, afford) = &v.specials[k];
            let r = rt.entry(wi).or_default();
            // ESpecialWeaponState ESWS_CantAfford: grey (the native colour is not decoded,
            // NOT from data), ReadyToBuy / ReadyToUse: HUD green
            let base = if *red {
                self.team_colors[1]
            } else if !*afford {
                [0.45, 0.45, 0.45, 1.0]
            } else {
                [0.505433, 1.0, 0.612066, 1.0]
            };
            let a = if *selected { a_on } else { a_off };
            let col = [base[0], base[1], base[2], a];
            match icon.as_deref().and_then(|p| ui.world.find(p)) {
                Some(tex) => {
                    r.simple.insert(si_icon, SimpleRt { image: Some(tex), color: Some(col), hidden: Some(false), ..Default::default() });
                }
                None => {
                    r.simple.insert(si_icon, SimpleRt { hidden: Some(true), ..Default::default() });
                }
            }
            match uses {
                Some(n) => {
                    r.simple.insert(si_label, SimpleRt { text: Some(n.to_string()), color: Some(col), hidden: Some(false), ..Default::default() });
                }
                None => {
                    r.simple.insert(si_label, SimpleRt { hidden: Some(true), ..Default::default() });
                }
            }
        }
        // gameplay hint (UI_GameplayHints, the key name from UI_Keybinds): user's screenshot
        // centred at y 588 of 720 in GamerTagFont, HUD green
        let hint_key = if !crate::settings::flag("PlayerDetails:GameHints") { None } else { match v.hint {
            Some("deploy_drone") => Some(("UI_GameplayHints.GameplayHints.DeployDrone", "ZUM ENTSENDEN EINER DROHNE")),
            Some("recon_mark") => Some(("UI_GameplayHints.GameplayHints.ReconDroneMarkTarget", "HALTEN, UM EIN ZIEL ZU MARKIEREN")),
            Some("strike_zoom") => Some(("UI_GameplayHints.GameplayHints.AirStrikeZoom", "ZUM ZOOMEN")),
            _ if v.c4_hint => Some(("UI_GameplayHints.GameplayHints.C4Detonate", "ZUM DETONIEREN VON C4")),
            _ => None,
        } };
        if let (Some((loc_key, fallback)), Some(wi)) = (hint_key, self.objectives) {
            // zoom: EKBC_AltFire (right mouse button)
            let key = if v.hint == Some("strike_zoom") { ui.loc.get("UI_Keybinds.Keybinds.RightMouseButton").unwrap_or("RMT") } else { ui.loc.get("UI_Keybinds.Keybinds.LeftMouseButton").unwrap_or("LMT") };
            let text = ui.loc.get(loc_key).unwrap_or(fallback);
            let font = ui.world.find("UI_Fonts.HUD.GamerTagFont");
            rt.entry(wi).or_default().extra_text.push((format!("({key}) {text}"), font, [640.0, 580.0], 0.72, [0.505433, 1.0, 0.612066, 1.0], true));
        }
        // HUD mode (GCUIObject_InfantryPlacer / GCUIObject_VehiclePlacer by vehicle type, the
        // seat placers by seat): HUMVEE = VehiclePlacerJeep (+ Common), drone = AllDrones +
        // GroundAssault
        let mut show: Vec<&str> = Vec::new();
        let mut seat_placers: Vec<String> = Vec::new();
        if let Some(seat) = v.vehicle_seat {
            show.push("VehiclePlacerCommon");
            // the placers whose VehicleTypes name the vehicle's class or a parent (HUMVEE:
            // VehiclePlacerJeep, tank: VehiclePlacerTank, ...) and their Seat<n>Placer
            let mut found = false;
            let mut parents: Vec<String> = Vec::new();
            for (placer, class) in &self.placer_types {
                if !placer.contains('.') && v.vehicle_classes.iter().any(|c| c.eq_ignore_ascii_case(class)) {
                    show.push(placer.as_str());
                    parents.push(placer.clone());
                    found = true;
                }
            }
            if !found {
                show.push("VehiclePlacerJeep");
                parents.push("VehiclePlacerJeep".into());
            }
            // the seat placer of the player's seat by SeatIndex (Seat1Placer 0, Seat2Placer 1);
            // the other seats' placers are visible by default in the data and go (they showed
            // the driver's reticule to the gunner)
            for p in &parents {
                let mine = self.seat_index.iter().filter(|(k, _)| k.starts_with(&format!("{p}."))).find(|(_, i)| **i as usize == seat).map(|(k, _)| k.clone());
                let mine = mine.or_else(|| {
                    let k = format!("{p}.Seat{}Placer", seat.min(1) + 1);
                    self.placers.contains_key(&k).then_some(k)
                });
                for (k, _) in self.seat_index.iter().filter(|(k, _)| k.starts_with(&format!("{p}."))) {
                    if let Some(&wi) = self.placers.get(k) {
                        rt.entry(wi).or_default().hidden = Some(true);
                    }
                }
                if let Some(m) = mine {
                    seat_placers.push(m);
                }
            }
        }
        if v.drone.is_some() {
            for (placer, class) in &self.placer_types {
                if v.drone_classes.iter().any(|c| c == class) && !placer.contains('.') {
                    show.push(placer.as_str());
                }
            }
        }
        // Hellfire UAV: the Predator placer (left / right bars, laser armed, battery, missiles
        // left: PC_U_PlayerHudScreen.VehiclePlacerPredator)
        if v.predator.is_some() {
            show.push("VehiclePlacerPredator");
        }
        if !show.is_empty() {
            for &k in &self.infantry {
                rt.entry(k).or_default().hidden = Some(true);
            }
            for name in show.into_iter().chain(seat_placers.iter().map(|s| s.as_str())) {
                if let Some(&k) = self.placers.get(name) {
                    rt.entry(k).or_default().hidden = Some(false);
                }
            }
        }
        // gunsight overlays (MilitaryHUD sub-placers: STINGER ARMED, HE ON, range box ...) only in
        // the first-person view; the flares panel (VehiclePlacerMissileDefeat) has nothing behind
        // it yet
        if v.vehicle_seat.is_some() {
            for &m in &self.military {
                rt.entry(m).or_default().hidden = Some(!v.vehicle_first_person);
            }
            if let Some(&k) = self.placers.get("VehiclePlacerMissileDefeat") {
                rt.entry(k).or_default().hidden = Some(true);
            }
            // RangeToTarget: "888.88 M" placeholder -> the aim distance (format from the
            // placeholder; NOT from data: "---" without a hit)
            let text = v.vehicle_range.map(|r| format!("{r:.2} M")).unwrap_or_else(|| "---".into());
            for (wi, labels) in &self.range_labels {
                let r = rt.entry(*wi).or_default();
                for &l in labels {
                    r.simple.insert(l, SimpleRt { text: Some(text.clone()), ..Default::default() });
                }
            }
        }
        // seat occupancy (native GCUIObject_VehicleOccupancy, NOT decoded): only the player's
        // seat is filled, its gamer tag "Du" like the kill crawl, SeatLabel
        // UI_HUD.VehicleHUD.DriverSeat / PassSeat, no voice icons
        if let (Some((wi, seats, label)), Some(seat)) = (&self.occupancy, v.vehicle_seat) {
            let r = rt.entry(*wi).or_default();
            for (k, (filled, tag, voip)) in seats.iter().enumerate() {
                let mine = k == seat.min(1);
                for &i in filled {
                    r.simple.insert(i, SimpleRt { hidden: Some(!mine), ..Default::default() });
                }
                for &i in tag {
                    r.simple.insert(i, SimpleRt { hidden: Some(!mine), text: mine.then(|| "Du".to_string()), ..Default::default() });
                }
                for &i in voip {
                    r.simple.insert(i, SimpleRt { hidden: Some(true), ..Default::default() });
                }
            }
            let key = if seat == 0 { "UI_HUD.VehicleHUD.DriverSeat" } else { "UI_HUD.VehicleHUD.PassSeat" };
            if let Some(t) = ui.loc.get(key) {
                for &i in label {
                    r.simple.insert(i, SimpleRt { text: Some(t.to_string()), ..Default::default() });
                }
            }
        }
        // HUMVEE gauges: SprintMeter = sprint time left, the BoostOn message while sprinting;
        // FireModeProgress[0] = gun heat (the native update is not decoded: NOT from data)
        if let Some((left, boosting, heat, _over)) = v.vehicle_gauges {
            if let Some((wi, meter, msg)) = &self.speedo {
                let r = rt.entry(*wi).or_default();
                for &m in meter {
                    r.simple.insert(m, SimpleRt { progress: Some(left), ..Default::default() });
                }
                for &m in msg {
                    r.simple.insert(m, SimpleRt { hidden: Some(!boosting), ..Default::default() });
                }
            }
            if let Some((wi, bar)) = &self.overheat {
                let r = rt.entry(*wi).or_default();
                for &b in bar {
                    r.simple.insert(b, SimpleRt { progress: Some(heat), ..Default::default() });
                }
            }
        }
        // drone lock-on box (GCUIObject_TargetingBox bTargetDrawBox, T_DefaultWhite lines; the
        // colour is native: HUD green as in the user's video, 2 units thick NOT from data)
        if let (Some(w), Some(&k), Some((lo, hi))) = (self.white, self.placers.get("VehiclePlacerAllDrones"), v.lock_box) {
            let r = rt.entry(k).or_default();
            let (x0, y0, x1, y1) = (640.0 + lo[0], 360.0 + lo[1], 640.0 + hi[0], 360.0 + hi[1]);
            let c = [0.505433, 1.0, 0.612066, 1.0];
            let t = 2.0;
            r.extra.push((w, [x0, y0, x1, y0 + t], c));
            r.extra.push((w, [x0, y1 - t, x1, y1], c));
            r.extra.push((w, [x0, y0, x0 + t, y1], c));
            r.extra.push((w, [x1 - t, y0, x1, y1], c));
        }
        // launcher lock-on (GCLockOnTargeter: BigRectangle = LockOnBigRectangleMIC, the target in
        // the SmallRectangle MAT_RD M_Lockon material - drawn as lines here). NOT from data: the
        // colours (HUD green as the drone box; acquiring blinks faster as the lock fills, locked
        // = red) and the line thickness; the material is not rendered
        if let (Some(w), Some(&k), Some(wl)) = (self.white, self.infantry.first(), v.weapon_lock.as_ref()) {
            let r = rt.entry(k).or_default();
            let line = |r: &mut WidgetRt, lo: [f32; 2], hi: [f32; 2], c: [f32; 4], t: f32| {
                r.extra.push((w, [lo[0], lo[1], hi[0], lo[1] + t], c));
                r.extra.push((w, [lo[0], hi[1] - t, hi[0], hi[1]], c));
                r.extra.push((w, [lo[0], lo[1], lo[0] + t, hi[1]], c));
                r.extra.push((w, [hi[0] - t, lo[1], hi[0], hi[1]], c));
            };
            let (lo, hi) = wl.big;
            line(r, [640.0 + lo[0], 360.0 + lo[1]], [640.0 + hi[0], 360.0 + hi[1]], [0.505433, 1.0, 0.612066, 0.8], 2.0);
            if let Some((tl, br, prog, locked)) = wl.target {
                let c = if locked {
                    [1.0, 0.1, 0.05, 1.0]
                } else {
                    let on = (v.clock * (4.0 + 12.0 * prog)).fract() < 0.5;
                    [0.505433, 1.0, 0.612066, if on { 1.0 } else { 0.35 }]
                };
                line(r, tl, br, c, if locked { 3.0 } else { 2.0 });
            }
        }
        // recon drone markers (GCUIObject_ReconDroneMarker in InfantryOrVehicle_Bottom: marked
        // enemies in every view; the drone's candidates with the lock-on SmallRectangle material
        // corners). NOT from data: marked size 45 units (user's video)
        if let Some(k) = self.recon_widget {
            if !v.recon.is_empty() {
                let r = rt.entry(k).or_default();
                r.hidden = Some(false);
                for (p, marked) in &v.recon {
                    let (tex, rect) = if *marked {
                        (self.recon_tex.0, [p.x - 22.5, p.y - 22.5, p.x + 22.5, p.y + 22.5])
                    } else {
                        (self.recon_tex.1, [p.x - 40.0, p.y - 49.0, p.x + 49.0, p.y + 40.0])
                    };
                    if let Some(t) = tex {
                        r.extra.push((t, rect, [1.0, 1.0, 1.0, 1.0]));
                    }
                }
            }
        }
        // airstrike target marker under the Predator placer's widgets
        if let (Some(w), Some(&k)) = (self.white, self.placers.get("VehiclePlacerPredator")) {
            if !v.strike_marker.is_empty() {
                let r = rt.entry(k).or_default();
                for (c, len, width, ang) in &v.strike_marker {
                    r.extra_rot.push((w, [c.x - len * 0.5, c.y - width * 0.5, c.x + len * 0.5, c.y + width * 0.5], [1.0, 1.0, 1.0, 0.75], *ang));
                }
            }
        }
        if let (Some((wi, objs)), Some((fuel, _))) = (&self.battery_pred, v.predator) {
            let r = rt.entry(*wi).or_default();
            r.hidden = Some(false);
            if let Some(&last) = objs.last() {
                r.simple.insert(last, SimpleRt { progress: Some(fuel.clamp(0.0, 1.0)), ..Default::default() });
            }
        }
        // drone battery (GCUIObject_DroneBatteryLife): the fill image scaled to the charge left
        if let (Some((wi, objs)), Some((charge, _))) = (&self.battery, v.drone) {
            let r = rt.entry(*wi).or_default();
            r.hidden = Some(false);
            if let Some(&last) = objs.last() {
                r.simple.insert(last, SimpleRt { progress: Some(charge.clamp(0.0, 1.0)), ..Default::default() });
            }
        }
        // full map (M held): the level capture over the MinimapVolume bounds, 1260 units wide,
        // centred (user's screenshot: 8..1236 of 1260 px, level aspect); objective letters, team
        // mates, the player arrow turned to the view; the minimap is hidden meanwhile
        let mut full_shown = false;
        if let (Some(fm), Some(mm)) = (&self.full_map, &v.minimap) {
            if let (Some((sq, b)), Some(cap)) = (mm.full, fm.capture) {
                full_shown = true;
                for &c in &fm.children {
                    rt.entry(c).or_default().hidden = Some(true);
                }
                let r = rt.entry(fm.wi).or_default();
                r.hidden = Some(false);
                let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
                // inside 1260 x 440 centred at y 366 (user's screenshot: 8..1236 x 137..545 of
                // 1260 x 670), level aspect kept
                let w = 1260.0;
                let h = (w * bh / bw.max(1.0)).min(440.0);
                let w = h * bw / bh.max(1.0);
                let rect = [640.0 - w * 0.5, 366.0 - h * 0.5, 640.0 + w * 0.5, 366.0 + h * 0.5];
                let uv = [(b[0] - sq[0]) / sq[2], (b[1] - sq[1]) / sq[2], (b[2] - sq[0]) / sq[2], (b[3] - sq[1]) / sq[2]];
                r.extra_uv.push((cap, rect, uv, [1.0; 4]));
                let to = |p: [f32; 2]| [rect[0] + (p[0] - b[0]) / bw * w, rect[1] + (p[1] - b[1]) / bh * h];
                if let Some((tex, size)) = fm.friend {
                    for &f in &mm.friends {
                        let c = to(f);
                        r.extra.push((tex, [c[0] - size * 0.5, c[1] - size * 0.5, c[0] + size * 0.5, c[1] + size * 0.5], [1.0; 4]));
                    }
                }
                if let Some((tex, size)) = fm.enemy.or(self.minimap.as_ref().and_then(|m| m.enemy)) {
                    for &f in &mm.enemies {
                        let c = to(f);
                        r.extra.push((tex, [c[0] - size * 0.5, c[1] - size * 0.5, c[0] + size * 0.5, c[1] + size * 0.5], ENEMY_BLIP));
                    }
                }
                for &(p, letter, owner) in &mm.objectives {
                    let Some(tex) = fm.objectives[letter.min(3)] else { continue };
                    let c = to(p);
                    let hs = fm.objective_size * 0.5;
                    let color = match owner {
                        Some(t) => self.team_colors[rel_team(t)],
                        None => [1.0; 4],
                    };
                    r.extra.push((tex, [c[0] - hs, c[1] - hs, c[0] + hs, c[1] + hs], color));
                }
                if let Some((tex, size)) = fm.player {
                    let c = to(mm.center);
                    let hs = size * 0.5;
                    // the icon points up; UE yaw 0 = +X = screen right, clockwise on screen
                    r.extra_rot.push((tex, [c[0] - hs, c[1] - hs, c[0] + hs, c[1] + hs], [1.0; 4], mm.yaw + std::f32::consts::FRAC_PI_2));
                }
            }
        }
        if !full_shown {
            if let Some(fm) = &self.full_map {
                rt.entry(fm.wi).or_default().hidden = Some(true);
            }
        } else {
            if let Some(mw) = &self.minimap {
                rt.entry(mw.wi).or_default().hidden = Some(true);
            }
            if let Some(e) = self.minimap_effects {
                rt.entry(e).or_default().hidden = Some(true);
            }
        }
        // crosshair: four dashes (Reticule_Tank_VerticalDash 8x32 / HorizontalDash 32x8, length x
        // CrosshairImageSizeLengthMultiplier 0.35) around the centre, HUD green; hit notify
        // TX_Reticule_Hit_v2 at HitEnemyImageSize 64 fading over HitEnemyFadeTime
        if let Some((wi, tv, th, lm, tm, hit)) = self.crosshair {
            let r = rt.entry(wi).or_default();
            r.hidden = Some(false);
            let (cx, cy) = (640.0, 360.0);
            // in a vehicle the seat placer brings its own reticule: no infantry dashes
            if let Some(rad) = v.crosshair.filter(|_| v.vehicle_seat.is_none()) {
                let (len, thick) = (32.0 * lm, 8.0 * tm);
                let c = [0.505433, 1.0, 0.612066, 1.0];
                r.extra.push((tv, [cx - thick * 0.5, cy - rad - len, cx + thick * 0.5, cy - rad], c));
                r.extra.push((tv, [cx - thick * 0.5, cy + rad, cx + thick * 0.5, cy + rad + len], c));
                r.extra.push((th, [cx - rad - len, cy - thick * 0.5, cx - rad, cy + thick * 0.5], c));
                r.extra.push((th, [cx + rad, cy - thick * 0.5, cx + rad + len, cy + thick * 0.5], c));
            }
            if let (Some(ht), true) = (hit, v.hit_marker > 0.0) {
                let a = (v.hit_marker / crate::combat::HIT_FADE).clamp(0.0, 1.0);
                r.extra.push((ht, [cx - 32.0, cy - 32.0, cx + 32.0, cy + 32.0], [1.0, 1.0, 1.0, a]));
            }
            // flash bang: full white, fading over the last 1.5 s (NOT from data)
            if let (Some(w), true) = (self.white, v.blinded > 0.0) {
                let a = (v.blinded / 1.5).clamp(0.0, 1.0);
                r.extra.push((w, [0.0, 0.0, 1280.0, 720.0], [1.0, 1.0, 1.0, a]));
            }
        }
        // MiniMapEffects (M_MiniMap_ColorEffect, white scan lines at alpha 0.1): its stored Position
        // (0.446, 0.927 of the viewport) would put it off screen; trace t3 frame 2700 draws the
        // lines next to the map (x 58..243 / y 442..685 in 1280x720, animated: the vertical
        // offset is not decoded) - drawn at their absolute positions (NOT from data: no motion)
        if let Some(wi) = self.minimap_effects {
            rt.entry(wi).or_default().origin = Some([0.0, 0.0]);
        }
        // the vehicle seats' reticules at their absolute positions (the tank driver / main gun
        // reticule Seat1Placer.Reticule was drawn shifted by its widget corner 390,260 to the
        // bottom right, over the vehicle icon: "crosshair missing", user 2026-10-21 23:56)
        for &wi in &self.seat_reticules {
            rt.entry(wi).or_default().origin = Some([0.0, 0.0]);
        }
        // round pills: filled ones for the rounds won
        if let Some((wi, f, e)) = &self.pills {
            let r = rt.entry(*wi).or_default();
            // the data parks the mode-specific parts below the screen (y 1000..1040); the game's
            // code places them next to the team labels. Frontline layout as drawn in trace t3
            // frame 2700 (1280x720 units): ProgressBarLine_2Boxes at (91, 464.8) / (91, 484.8)
            // 128x16; score bars (T_DefaultWhite, GCUISimpleProgressBar Size 100x8 with
            // BarShrinkPct 0.96 / 0.5 -> 96 x 4) from x 107 at y 468.8 / 488.8; round squares
            // Token_Square 16x16 at x 209 / 225, y 461.8 / 481.8 (alpha 0.75), Token_Fill on them
            // for the rounds won. Colours from the objects' DrawColor (USA 0.505 1 0.612,
            // KVA 0.867 0.078 0.087).
            for &si in f.iter().chain(e.iter()) {
                r.simple.insert(si, SimpleRt { hidden: Some(true), ..Default::default() });
            }
            let cols = [[0.505433, 1.0, 0.612066, 1.0], [0.867136, 0.078057, 0.086901, 1.0]];
            let find = |p: &str| ui.world.find(p);
            let (line, white, square, fill) = (find("UI_PlayerHUD.Textures.HUD.ProgressBarLine_2Boxes"), find("UI_Common.Textures.T_DefaultWhite"), find("UI_PlayerHUD.Textures.HUD.Token_Square"), find("UI_PlayerHUD.Textures.HUD.Token_Fill"));
            for t in 0..2 {
                let dy = t as f32 * 20.0;
                let c = cols[t];
                if let Some(l) = line {
                    r.extra.push((l, [91.0, 464.8 + dy, 219.0, 480.8 + dy], c));
                }
                let p = if v.score_limit > 0 { (v.score[team_row(t)] as f32 / v.score_limit as f32).clamp(0.0, 1.0) } else { 0.0 };
                if let (Some(w), true) = (white, p > 0.0) {
                    r.extra.push((w, [107.0, 468.8 + dy, 107.0 + 96.0 * p, 472.8 + dy], c));
                }
                for k in 0..2 {
                    let x = 209.0 + k as f32 * 16.0;
                    if let Some(sq) = square {
                        r.extra.push((sq, [x, 461.8 + dy, x + 16.0, 477.8 + dy], [c[0], c[1], c[2], 0.75]));
                    }
                    if let (Some(fl), true) = (fill, (k as i32) < v.rounds[team_row(t)]) {
                        r.extra.push((fl, [x, 461.8 + dy, x + 16.0, 477.8 + dy], c));
                    }
                }
            }
        }
        // GCUIObject_BannerMessage (iMessage1 at 640 / 255, Size 0.68; iMessage2 at 640 / 277.4,
        // Size 0.48; HUDFont_Green, centred). MainBannerAnimation_Fast / _Slow: iMessage1 scales
        // 10 -> 0.68 and fades in over 0.2 s, iMessage2 fades in 0.2-0.5 s, all fade out
        // 1.0-1.3 s (slow: 2.2-2.5 s). The widget itself stays hidden (its placeholder texts),
        // the lines are drawn with the objectives widget.
        if let (Some((m1, m2, t0, slow)), Some(wi)) = (&v.banner_msg, self.objectives) {
            let t = v.clock - t0;
            let out = if *slow { 2.2 } else { 1.0 };
            let fade_out = if t > out { (1.0 - (t - out) / 0.3).max(0.0) } else { 1.0 };
            let font = ui.world.find("UI_Fonts.HUD.HUDFont_Green");
            let fh = font.and_then(|f| ui.font(f)).map(|f| f.chars.iter().map(|g| g.h).max().unwrap_or(32) as f32).unwrap_or(32.0);
            let r = rt.entry(wi).or_default();
            let k = (t / 0.2).clamp(0.0, 1.0);
            let s1 = 10.0 + (0.68 - 10.0) * k;
            let a1 = k * fade_out;
            if a1 > 0.0 && !m1.is_empty() {
                // scaled about the line's centre
                r.extra_text.push((m1.clone(), font, [640.0, 255.0 + (0.68 - s1) * fh * 0.5], s1, [1.0, 1.0, 1.0, a1], true));
            }
            let a2 = ((t - 0.2) / 0.3).clamp(0.0, 1.0) * fade_out;
            if a2 > 0.0 && !m2.is_empty() {
                r.extra_text.push((m2.clone(), font, [640.0, 277.4], 0.48, [1.0, 1.0, 1.0, a2], true));
            }
        }
        if let (Some(b), Some(wi)) = (&v.banner, self.objectives) {
            let font = ui.world.find("UI_Fonts.HUD.GamerTagFont");
            rt.entry(wi).or_default().extra_text.push((b.clone(), font, [640.0, 250.0], 1.4, [0.505433, 1.0, 0.612066, 1.0], true));
        }
        let t = v.time_left.max(0.0).ceil() as i32;
        for &(wi, si, role) in &self.roles {
            if let Role::Progress(_) = role {
                // drawn at the frontline layout below (score_bars)
                rt.entry(wi).or_default().simple.insert(si, SimpleRt { hidden: Some(true), ..Default::default() });
                continue;
            }
            if role == Role::ObjectiveText {
                // GCUIObject_GameObjectives ActiveObjectiveDescription: "%s WIRD GESICHERT" etc.
                let r = rt.entry(wi).or_default();
                match v.objective_msg {
                    Some((k, l)) => {
                        let name = ui.loc.get(&format!("gcgame.Objective_GameMode.LocalizedObjectiveName[{l}]")).unwrap_or("").to_string();
                        let st = ui.loc.get(&format!("gcgame.Objective_GameMode.LocalizedObjectiveStatus[{k}]")).unwrap_or("%s").replace("%s", &name);
                        r.simple.insert(si, SimpleRt { text: Some(st), hidden: Some(false), ..Default::default() });
                    }
                    None => {
                        r.simple.insert(si, SimpleRt { hidden: Some(true), ..Default::default() });
                    }
                }
                continue;
            }
            let text = match role {
                Role::Timer => format!("{}:{:02}", t / 60, t % 60),
                // first row = the player's team (USA at the start; KVA after a team switch)
                Role::TeamUsa | Role::TeamKor => {
                    let usa = (role == Role::TeamUsa) == (crate::combat::player_team() == crate::combat::TEAM_USA);
                    if usa { ui.loc.get("UI_HUD.GCUIObject_GameScore.TeamUSA").unwrap_or("USA").to_string() } else { ui.loc.get("UI_HUD.GCUIObject_GameScore.TeamKOR").unwrap_or("KOR").to_string() }
                }
                Role::ScoreUsa => v.score[team_row(0)].to_string(),
                Role::ScoreKor => v.score[team_row(1)].to_string(),
                Role::Kp => {
                    // red while counting down after spending (video 81-83.5 s)
                    let text = (v.kp_shown.round() as i32).to_string();
                    let color = (v.kp_shown > v.kp as f32 + 0.5).then_some(self.team_colors[1]);
                    rt.entry(wi).or_default().simple.insert(si, SimpleRt { text: Some(text), color, ..Default::default() });
                    continue;
                }
                Role::Ammo => v.ammo.to_string(),
                Role::Grenade(i) => v.grenades[i].to_string(),
                Role::AirstrikeLeft => v.predator.map(|p| p.1.max(0).to_string()).unwrap_or_default(),
                Role::ObjectiveText | Role::Progress(_) => String::new(),
            };
            rt.entry(wi).or_default().simple.insert(si, SimpleRt { text: Some(text), ..Default::default() });
        }
        // kill cam overlay (PC_U_KillCamScreen): DelayCounter Message "ENTSTEHUNG IN:"
        // (UI_InGameMenus Respawn SpawnDelayString; box 0.338..0.662 x 0.181..0.236 of the
        // viewport, HUDFont_Green at 0.72) and Counter (0.201..0.299, scale 1), both centred,
        // DrawColor (1, 0.048, 0.055) at Opacity 0.8; GCUIObject_DeathMessage label at (640, 500),
        // GamerTagFont 0.72, HUD green: GCDeathMessage Suicide / YouWereKilledBy + name
        if let (Some((killer, left)), Some(wi)) = (&v.death, self.objectives) {
            // the kill cam scene replaces the player HUD
            for &k in &self.death_hide {
                rt.entry(k).or_default().hidden = Some(true);
            }
            let r = rt.entry(wi).or_default();
            r.extra.clear();
            r.extra_text.clear();
            for (_, si) in r.simple.iter_mut() {
                si.hidden = Some(true);
            }
            r.hidden = Some(false);
            let big = ui.world.find("UI_Fonts.HUD.HUDFont_Green");
            let tag = ui.world.find("UI_Fonts.HUD.GamerTagFont");
            let fh = |f: Option<GRef>| f.and_then(|f| ui.font(f)).map(|f| f.chars.iter().map(|g| g.h).max().unwrap_or(32) as f32).unwrap_or(32.0);
            let red = [1.0, 0.047776, 0.054592, 0.8];
            let msg = ui.loc.get("UI_InGameMenus.Respawn.SpawnDelayString").unwrap_or("ENTSTEHUNG IN:").to_string();
            let y_msg = (0.18055555 + 0.2361111) * 0.5 * 720.0 - fh(big) * 0.72 * 0.5;
            r.extra_text.push((msg, big, [640.0, y_msg], 0.72, red, true));
            let y_cnt = (0.20119175 + 0.29880825) * 0.5 * 720.0 - fh(big) * 0.5;
            r.extra_text.push((left.to_string(), big, [640.0, y_cnt], 1.0, red, true));
            let line = match killer {
                Some((_, true)) | None => ui.loc.get("GCGame.GCDeathMessage.Suicide").unwrap_or("Sie haben Selbstmord begangen").to_string(),
                Some((name, false)) => format!("{} {}", ui.loc.get("GCGame.GCDeathMessage.YouWereKilledBy").unwrap_or("SIE WURDEN GETÖTET VON"), name),
            };
            r.extra_text.push((line, tag, [640.0, 500.0], 0.72, [0.505433, 1.0, 0.612066, 1.0], true));
        }
        // HUD preferences (U_HUDPrefs)
        for &(k, wi) in &self.prefs {
            if !crate::settings::flag(k) {
                rt.entry(wi).or_default().hidden = Some(true);
            }
        }
        // FrameRateToggle: frames per second top right in GamerTagFont, HUD green (placement NOT
        // from data)
        if crate::settings::flag("PlayerDetails:FrameRateToggle") {
            if let Some(wi) = self.objectives {
                let font = ui.world.find("UI_Fonts.HUD.GamerTagFont");
                rt.entry(wi).or_default().extra_text.push((format!("{:.0} FPS", crate::settings::fps()), font, [1230.0, 8.0], 0.6, [0.505433, 1.0, 0.612066, 1.0], true));
            }
        }
        // killed by the blast of a bot's nuke: no HUD at all (crosshair, ammo, minimap,
        // banners) while the kill cam shows the ragdoll and the white-out / far view follow
        // (user 2026-10-23 00:14; NOT from data)
        if crate::nuke::hide_hud() {
            for &k in &self.death_hide {
                rt.entry(k).or_default().hidden = Some(true);
            }
            if let Some(wi) = self.objectives {
                rt.entry(wi).or_default().hidden = Some(true);
            }
        }
        rt
    }
}

/// HUD colour slot of a team: 0 (the data's USA / friendly colour) for the player's team, 1
/// (KOR / enemy) for the other - after a team switch the KVA are the friendly side. NOT from
/// data: whether the game colours by side or by nation is not decoded.
fn rel_team(t: u8) -> usize {
    (t != crate::combat::player_team()) as usize
}

/// Team index (score / rounds arrays: 0 USA, 1 KVA) shown on HUD score row `row` (0 = top row =
/// the player's team).
fn team_row(row: usize) -> usize {
    (row ^ crate::combat::player_team() as usize) & 1
}
