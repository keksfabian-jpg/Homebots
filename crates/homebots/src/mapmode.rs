//! Game modes of a map folder (creategame.rs, menu.rs, mapcheck.rs).
//!
//! The persistent map (FL-<map>.map) streams one gameplay sublevel per mode (its
//! LevelStreamingKismet list, main.rs level_files): <map>_FrontlinesData for Bodenkontrolle
//! (GameMode_FocusedFrontline, Objective_GameMode_VolumeHold, GameMode_FrontlineVolume,
//! FL_Settings_Content) and TDM_<map>[_c] for Team Deathmatch (GameMode_TDMFrontline with
//! team-agnostic spawn points, TDM_Settings_Content). Crossroads has both; Overpass ("HIGHWAY",
//! DLC map) only tdm_Overpass: FL-Overpass.map streams tdm_Overpass and Overpass_bcsettings,
//! there is no Overpass_FrontlinesData and FL-Overpass holds only PlayerSpawnManagerTDM_Content.
use std::path::{Path, PathBuf};

/// TDM / TDM,BC (DefaultEngine.ini GameModeTypes GameModePrefix); FL / FL,BC = Bodenkontrolle.
pub fn is_tdm(mode: &str) -> bool {
    mode.split(',').next().is_some_and(|m| m.trim().eq_ignore_ascii_case("TDM"))
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Bodenkontrolle's gameplay sublevel: <map>_FrontlinesData; Spillway's is
/// "FL-Spillway-FrontlinesData" (hyphen).
pub fn is_fl_data(stem: &str) -> bool {
    let s = stem.to_ascii_lowercase();
    s.ends_with("_frontlinesdata") || s.ends_with("-frontlinesdata")
}

/// Team Deathmatch's gameplay sublevel: TDM_<map>[_c], tdm_<map>_<area>; Culdesac's is
/// "tdm-culdesac_gastation" (hyphen).
pub fn is_tdm_data(stem: &str) -> bool {
    let s = stem.to_ascii_lowercase();
    s.starts_with("tdm_") || s.starts_with("tdm-")
}

/// Gameplay sublevels whose spawn data is incomplete (HB_MAPCHECK "spawn data complete: false"):
/// (folder, mode). alcatraz_FrontlinesData (194 KB, DLC map) has 3 GameMode_FocusedFrontlines
/// and 13 Objective_GameMode_VolumeHold but only 6 GameMode_PlayerStarts: front lines 0 and 2
/// have no infantry start for either team, the middle one 1 USA / 2 KVA. Its tdm_alcatraz
/// (24 starts) is complete. Checking this at runtime would mean opening every gameplay
/// sublevel when the create-game screen opens, so the result of the check is listed here.
const INCOMPLETE: [(&str, &str); 1] = [("alcatraz", "FL")];

/// The folder has the gameplay sublevel of `mode` (file names only: cheap enough for the
/// create-game screen), and it is not listed as incomplete.
pub fn has_data(folder: &Path, mode: &str) -> bool {
    let tdm = is_tdm(mode);
    let name = folder.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if INCOMPLETE.iter().any(|(f, m)| *f == name && is_tdm(m) == tdm) {
        return false;
    }
    std::fs::read_dir(folder)
        .map(|rd| {
            rd.flatten().map(|e| e.path()).any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("map")) && if tdm { is_tdm_data(&stem(&p)) } else { is_fl_data(&stem(&p)) })
        })
        .unwrap_or(false)
}

/// The match's level packages: the persistent map's streamed sublevels (main.rs level_files)
/// without the other mode's gameplay sublevel. Bodenkontrolle: everything but TDM_<map>_c (as
/// before); TDM: everything but <map>_FrontlinesData.
pub fn match_maps(folder: &Path, mode: &str) -> Vec<PathBuf> {
    let tdm = is_tdm(mode);
    crate::level_files(folder)
        .into_iter()
        .filter(|m| {
            let s = stem(m);
            if tdm { !is_fl_data(&s) } else { !is_tdm_data(&s) }
        })
        .collect()
}
