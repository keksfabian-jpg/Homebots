//! In-game scoreboard PC_U_InGameScoreboard (GCUIScene_InGameScoreboard), held on
//! EKBC_OpenScoreboard (DefaultInput BindingsPCGlobal Tab: "OpenInGameScoreboard | OnRelease
//! CloseInGameScoreboard"). The scene has the player's side on the left (FriendsScoreboard,
//! Friends{US,KPA}FlagImage) and the other side on the right (EnemyScoreboard,
//! Enemy{US,KPA}FlagImage); the flags follow the player's team (team switch). The rows are drawn
//! by the native GCUISimpleList_Scoreboard in the game: here as text in the header columns
//! (ClanLabel / PlayerNameLabel / ScoreIcon / KillsIcon / DeathsIcon), one row per
//! ListSelectionImage height. NOT from data: row font (GamerTagFont 0.6 like the kill crawl),
//! row colours, sorting by score, score = kills x KillGameScore tier 1 (10).
use bevy::prelude::*;
use std::collections::HashMap;
use upk::ui::{Scene, Ui, WidgetRt};

/// (name, team, score, kills, deaths, is the player)
#[derive(Resource, Default, Clone)]
pub struct ScoreRows(pub Vec<(String, u8, i32, i32, i32, bool)>);

pub fn collect(mut rows: ResMut<ScoreRows>, soldiers: Query<&crate::combat::Soldier>, pc: Option<Res<crate::combat::PlayerCombat>>, flow: Res<crate::menu::MatchFlow>) {
    if flow.phase != crate::menu::Phase::Playing && flow.phase != crate::menu::Phase::Spawn {
        if !rows.0.is_empty() {
            rows.0.clear();
        }
        return;
    }
    let mut v: Vec<(String, u8, i32, i32, i32, bool)> = soldiers.iter().map(|s| (s.name.clone(), s.team, s.kills * 10, s.kills, s.deaths, false)).collect();
    if let Some(pc) = pc {
        // the player's name as in the kill crawl (combat.rs "Du")
        v.push(("Du".into(), crate::combat::player_team(), pc.kills * 10, pc.kills, pc.deaths, true));
    }
    v.sort_by(|a, b| b.2.cmp(&a.2).then(a.4.cmp(&b.4)).then(a.0.cmp(&b.0)));
    rows.0 = v;
}

/// Runtime of the scoreboard scene: flags by the player's team, rows, player counts.
pub fn runtime(ui: &Ui, scene: &Scene, rows: &ScoreRows, vw: f32, vh: f32) -> HashMap<usize, WidgetRt> {
    let mut rt: HashMap<usize, WidgetRt> = HashMap::new();
    let usa = crate::combat::player_team() == crate::combat::TEAM_USA;
    let idx = |tag: &str| scene.widgets.iter().position(|w| w.tag == tag);
    for (tag, show) in [("FriendsUSFlagImage", usa), ("FriendsKPAFlagImage", !usa), ("EnemyUSFlagImage", !usa), ("EnemyKPAFlagImage", usa), ("LobbyTimer", false), ("MuteToggleLabel", false), ("MuteToggleCheckbox", false)] {
        if let Some(i) = idx(tag) {
            rt.entry(i).or_default().hidden = Some(!show);
        }
    }
    let font = ui.world.find("UI_Fonts.HUD.GamerTagFont");
    let (sx, sy) = (1280.0 / vw, 720.0 / vh);
    for (list, number, friends) in [("FriendsScoreboard", "FriendsNumberLabel", true), ("EnemyScoreboard", "EnemyNumberLabel", false)] {
        let Some(li) = idx(list) else { continue };
        let team = if friends { crate::combat::player_team() } else { crate::combat::player_team() ^ 1 };
        let mine: Vec<_> = rows.0.iter().filter(|r| r.1 == team).collect();
        // columns: the list's header widgets (children of the list)
        let child = |tag: &str| scene.widgets[li].children.iter().copied().find(|&c| scene.widgets[c].tag == tag).map(|c| scene.widgets[c].rect);
        let name_x = child("PlayerNameLabel").map(|r| r[0]).unwrap_or(scene.widgets[li].rect[0]);
        let cols: Vec<f32> = ["ScoreIcon", "KillsIcon", "DeathsIcon"].iter().filter_map(|t| child(t).map(|r| (r[0] + r[2]) * 0.5)).collect();
        let row_h = 23.5 * vh / 768.0; // ListSelectionImage ConsoleImageLabel height at 1366x768
        let top = scene.widgets[li].rect[1];
        let e = rt.entry(li).or_default();
        for (k, r) in mine.iter().enumerate() {
            let y = top + k as f32 * row_h + row_h * 0.15;
            if y + row_h > scene.widgets[li].rect[3] {
                break;
            }
            let col = if r.5 { [0.505433, 1.0, 0.612066, 1.0] } else { [1.0, 1.0, 1.0, 1.0] };
            e.extra_text.push((r.0.clone(), font, [name_x * sx, y * sy], 0.6, col, false));
            for (c, val) in cols.iter().zip([r.2, r.3, r.4]) {
                e.extra_text.push((val.to_string(), font, [c * sx, y * sy], 0.6, col, true));
            }
        }
        if let Some(n) = idx(number) {
            rt.entry(n).or_default().text = Some(mine.len().to_string());
        }
    }
    rt
}
