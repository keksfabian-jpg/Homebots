//! Damage feedback for the local player.
//!
//! - Direction of hits: PC_U_PlayerHudScreen InfantryOrVehicle_Bottom.HitDetector
//!   (GCUIObject_HitDetector, HitLocatorTexture UI_PlayerHUD.Textures.Hit_Locator). Its script
//!   class keeps MAX_HIT_LOCATORS = 3 HitLocatorDatum entries (HitOrigin, DirOfHit,
//!   HitTimeElapsed, HitLocationFadePercent) written round-robin (CurrHitLocatorDataIndex) and
//!   fades them over FadeDuration 4 s (Default__GCUIObject_HitDetector); the direction follows the
//!   view from the stored origin. hud.rs draws them.
//! - Hit sound: Default__GCChar_MultiPlayer_Content TakeHitSound
//!   A2_ProjctileImpact_C.SC_BulletImpact_Body_1P (2D).
//! - Low health: m_HealthSoundCue A2_CharacterMovement_C.SC_Health when the health falls below
//!   GCPlayerController HealthDuckingStart 30 (the duck group "HealthLow" starts there).
//!
//! Not done: the HealthPP_MP / ImpactPP_MP post-process materials (GCBasePostProcess
//! M_TakeDamage_FahSho_INST FlashEffect / HealthEffect; native driving code not decoded).
use bevy::prelude::*;

pub struct DmgFeedbackPlugin;

impl Plugin for DmgFeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DamageTaken>().add_systems(Update, update.after(crate::combat::CombatSet));
    }
}

/// Hits the player took this frame (combat.rs apply_damage): amount, origin (UE), explosive,
/// world damage (falling, out of bounds).
#[derive(Resource, Default)]
pub struct DamageTaken(pub Vec<(f32, Vec3, bool, bool)>);

/// GCUIObject_HitDetector MAX_HIT_LOCATORS
const MAX_HIT_LOCATORS: usize = 3;
/// Default__GCUIObject_HitDetector FadeDuration
const FADE_DURATION: f32 = 4.0;
/// GCPlayerController HealthDuckingStart
const HEALTH_DUCKING_START: f32 = 30.0;
const TAKE_HIT_SOUND: &str = "A2_ProjctileImpact_C.SC_BulletImpact_Body_1P";
const HEALTH_SOUND: &str = "A2_CharacterMovement_C.SC_Health";

#[derive(Default)]
struct State {
    /// HitOrigin (UE), HitTimeElapsed
    locators: [Option<(Vec3, f32)>; MAX_HIT_LOCATORS],
    cur: usize,
    low: bool,
}

fn update(time: Res<Time>, mut taken: ResMut<DamageTaken>, mut flow: ResMut<crate::menu::MatchFlow>, pc: Res<crate::combat::PlayerCombat>, pawns: Query<&crate::player::Pawn>, mut sfx: ResMut<crate::sfx::SfxQueue>, mut st: Local<State>) {
    let dt = time.delta_secs();
    let hits = std::mem::take(&mut taken.0);
    let Ok(p) = pawns.single() else {
        flow.hud.hit_locators.clear();
        return;
    };
    if p.dead {
        *st = State::default();
        flow.hud.hit_locators.clear();
        return;
    }
    let mut sound = false;
    for (amount, from, _explosive, world) in hits {
        if amount <= 0.0 {
            continue;
        }
        sound = true;
        if !world {
            st.cur = (st.cur + 1) % MAX_HIT_LOCATORS;
            let c = st.cur;
            st.locators[c] = Some((from, 0.0));
        }
    }
    // one hit sound per frame (shotgun pellets / several bullets)
    if sound {
        sfx.play(TAKE_HIT_SOUND, None);
    }
    let low = pc.health < HEALTH_DUCKING_START;
    if low && !st.low {
        sfx.play(HEALTH_SOUND, None);
    }
    st.low = low;
    let eye = p.eye_ue();
    let d = p.dir_ue();
    let fwd = Vec2::new(d.x, d.y).normalize_or_zero();
    // UE: x forward, y right (left-handed): right of the view = (-fwd.y, fwd.x)
    let right = Vec2::new(-fwd.y, fwd.x);
    flow.hud.hit_locators.clear();
    for l in st.locators.iter_mut() {
        let Some((from, t)) = l else { continue };
        *t += dt;
        if *t >= FADE_DURATION {
            *l = None;
            continue;
        }
        let to = Vec2::new(from.x - eye.x, from.y - eye.y);
        let a = to.dot(right).atan2(to.dot(fwd));
        flow.hud.hit_locators.push((a, 1.0 - *t / FADE_DURATION));
    }
}
