use jzon::{array, object, JsonValue};
use lazy_static::lazy_static;

use super::{databases, items};
use databases::csv::{self, Region};

lazy_static! {
    static ref CELLS: JsonValue = csv::table(Region::Jp, "beginner_mission");
    static ref SETTINGS: JsonValue = csv::table(Region::Jp, "beginner_mission_reward_setting");
    static ref REWARDS: JsonValue = csv::table(Region::Jp, "beginner_mission_reward");
    static ref CARD_LEVELS: JsonValue = csv::table(Region::Jp, "card_level");
    static ref SKILL_LEVELS: JsonValue = csv::table(Region::Jp, "card_skill_level");
}

// JP /api/mission + /api/mission/receive captures, 2024-03-31:
// all sheets accumulate progress; only reached sheets can be claimed. Line
// rewards are milestones in the number of completed lines, not named lines.
const LINES: [[i64; 3]; 8] = [
    [1, 2, 3], [4, 5, 6], [7, 8, 9], [1, 4, 7],
    [2, 5, 8], [3, 6, 9], [1, 5, 9], [3, 5, 7],
];

pub fn contains(id: i64) -> bool {
    CELLS.members().any(|c| c["masterMissionId"].as_i64() == Some(id))
}

pub fn level(id: i64) -> i64 {
    CELLS.members().find(|c| c["masterMissionId"].as_i64() == Some(id))
        .and_then(|c| c["level"].as_i64()).unwrap_or(0)
}

pub fn is_counter(id: i64) -> bool {
    contains(id) && matches!(databases::MISSION_LIST[id.to_string()]["conditionType"].as_i64(), Some(5 | 6 | 53 | 62 | 63))
}

pub fn ensure(missions: &mut JsonValue) {
    for cell in CELLS.members() {
        let id = cell["masterMissionId"].as_i64().unwrap();
        let target = databases::MISSION_LIST[id.to_string()]["conditionNumber"].as_i64().unwrap();
        if let Some(mission) = missions.members_mut().find(|m| m["master_mission_id"].as_i64() == Some(id)) {
            // The old login hook kept incrementing an already completed cell.
            // The client subtracts one to animate an unclaimed completion, so
            // over-target progress would incorrectly make its "before" sheet clear.
            let progress = if mission["status"].as_i64().unwrap_or(1) >= 2 { target }
                else { mission["progress"].as_i64().unwrap_or(0).clamp(0, target) };
            mission["progress"] = progress.into();
            continue;
        }
        missions.push(object! {
            master_mission_id: id, status: 1, progress: 0,
            expire_date_time: 0, not_visible: 0
        }).unwrap();
    }
}

pub fn set_progress(id: i64, progress: i64, missions: &mut JsonValue) -> bool {
    ensure(missions);
    let target = databases::MISSION_LIST[id.to_string()]["conditionNumber"].as_i64().unwrap();
    for mission in missions.members_mut() {
        if mission["master_mission_id"].as_i64() != Some(id) { continue; }
        if mission["status"].as_i64().unwrap_or(1) >= 2 { return false; }
        let progress = progress.max(mission["progress"].as_i64().unwrap_or(0)).min(target);
        mission["progress"] = progress.into();
        mission["status"] = if progress >= target { 2 } else { 1 }.into();
        return progress >= target;
    }
    false
}

pub fn advance(condition: i64, value: Option<i64>, amount: i64, missions: &mut JsonValue) -> JsonValue {
    ensure(missions);
    let mut cleared = array![];
    for cell in CELLS.members() {
        let id = cell["masterMissionId"].as_i64().unwrap();
        let mst = &databases::MISSION_LIST[id.to_string()];
        if mst["conditionType"].as_i64() != Some(condition) { continue; }
        if let Some(value) = value {
            if mst["conditionValues"][0].as_i64() != Some(value) { continue; }
        }
        let old = items::get_mission_status(id, missions)["progress"].as_i64().unwrap_or(0);
        if set_progress(id, old + amount, missions) { cleared.push(id).unwrap(); }
    }
    cleared
}

pub fn refresh(user: &JsonValue, missions: &mut JsonValue) -> JsonValue {
    ensure(missions);
    let mut cleared = array![];
    for cell in CELLS.members() {
        let id = cell["masterMissionId"].as_i64().unwrap();
        let mst = &databases::MISSION_LIST[id.to_string()];
        let value = mst["conditionValues"][0].as_i64().unwrap_or(0);
        let progress = match mst["conditionType"].as_i64().unwrap() {
            14 | 64 => {
                let skill = mst["conditionType"] == 64;
                user["card_list"].members().filter(|card| {
                    let master = super::custom_card::card_info(card["master_card_id"].as_i64().unwrap_or(0));
                    let curve = if skill { databases::CARD_RARITY[master["rarity"].to_string()]["masterCardSkillLevelId"].as_i64() }
                        else { master["masterCardLevelId"].as_i64() };
                    let levels = if skill { &*SKILL_LEVELS } else { &*CARD_LEVELS };
                    levels.members().any(|l| l["id"].as_i64() == curve && l["level"].as_i64() == Some(value)
                        && card[if skill { "skill_exp" } else { "exp" }].as_i64().unwrap_or(0) >= l["exp"].as_i64().unwrap())
                }).count() as i64
            }
            17 => user["card_list"].members().filter(|c| !c["evolve"].is_empty()).count() as i64,
            58 => user["character_list"].members().filter_map(|c| c["exp"].as_i64()).max().unwrap_or(0),
            30 | 60 => {
                let mut best = 0;
                for deck in user["deck_list"].members() {
                    let mut types = [0i64; 3];
                    let mut points = [0i64; 3];
                    for slot in deck["main_card_ids"].members() {
                        let Some(card) = user["card_list"].members().find(|c| c["id"] == *slot) else { continue; };
                        let master = super::custom_card::card_info(card["master_card_id"].as_i64().unwrap_or(0));
                        if let Some(t) = master["type"].as_usize().filter(|t| (1..=3).contains(t)) { types[t-1] += 1; }
                        let level = CARD_LEVELS.members().filter(|l| l["id"] == master["masterCardLevelId"]
                            && l["exp"].as_i64().unwrap() <= card["exp"].as_i64().unwrap_or(0))
                            .max_by_key(|l| l["level"].as_i64());
                        if let Some(l) = level {
                            for (i, name) in ["smile", "pure", "cool"].iter().enumerate() {
                                points[i] += master[*name].as_i64().unwrap_or(0) * l[format!("{}Ratio",name)].as_i64().unwrap_or(0) / 10000;
                            }
                        }
                    }
                    let p = if mst["conditionType"] == 30 {
                        let needed = mst["conditionValues"][1].as_i64().unwrap();
                        (types.iter().any(|n| *n >= needed)) as i64
                    } else { *points.iter().max().unwrap() };
                    best = best.max(p);
                }
                best
            }
            _ => continue,
        };
        if set_progress(id, progress, missions) { cleared.push(id).unwrap(); }
    }
    cleared
}

pub fn refresh_account(user: &JsonValue, missions: &mut JsonValue) -> JsonValue {
    let mut cleared = refresh(user, missions);
    // Existing accounts may have prepared transfer credentials before missions
    // were enabled. Merely requesting a code leaves an empty password.
    if let Some(uid) = user["user"]["id"].as_i64() {
        if super::userdata::has_transfer_password(uid) {
            for id in advance(25, None, 1, missions).members() { cleared.push(id.clone()).unwrap(); }
        }
    }
    cleared
}

fn sheet_cells(group: i64, level: i64) -> impl Iterator<Item = &'static JsonValue> {
    CELLS.members().filter(move |c| c["id"].as_i64() == Some(group) && c["level"].as_i64() == Some(level))
}

fn claimed(group: i64, level: i64, missions: &JsonValue) -> Vec<i64> {
    sheet_cells(group, level).filter(|c| {
        items::get_mission_status(c["masterMissionId"].as_i64().unwrap(), missions)["status"] == 3
    }).filter_map(|c| c["number"].as_i64()).collect()
}

pub fn claimable(id: i64, missions: &JsonValue) -> bool {
    let Some(cell) = CELLS.members().find(|c| c["masterMissionId"].as_i64() == Some(id)) else { return false; };
    CELLS.members().filter(|c| c["id"] == cell["id"] && c["level"].as_i64() < cell["level"].as_i64())
        .all(|c| items::get_mission_status(c["masterMissionId"].as_i64().unwrap(), missions)["status"] == 3)
}

pub fn home_status(missions: &JsonValue) -> (usize, bool) {
    let all = CELLS.members().all(|c| items::get_mission_status(c["masterMissionId"].as_i64().unwrap(), missions)["status"] == 3);
    let count = CELLS.members().filter(|c| {
        let id = c["masterMissionId"].as_i64().unwrap();
        items::get_mission_status(id, missions)["status"] == 2 && claimable(id, missions)
    }).count();
    (count, all)
}

pub fn new_rewards(before: &JsonValue, after: &JsonValue) -> Vec<JsonValue> {
    let mut settings: Vec<_> = SETTINGS.members().collect();
    // Captures return line milestones before the full-sheet reward.
    settings.sort_by_key(|s| (s["masterBeginnerMissionId"].as_i64(), s["level"].as_i64(),
        if s["number"] == 0 { 9 } else { s["number"].as_i64().unwrap() }));
    let mut result = Vec::new();
    for setting in settings {
        let group = setting["masterBeginnerMissionId"].as_i64().unwrap();
        let level = setting["level"].as_i64().unwrap();
        let number = setting["number"].as_i64().unwrap();
        // JP cross-sheet claims award bonuses only for sheets unlocked before
        // the request (recorded claims 8 and 11 in the replay fixture).
        let first = sheet_cells(group, level).next().unwrap()["masterMissionId"].as_i64().unwrap();
        if !claimable(first, before) { continue; }
        let achieved = |missions: &JsonValue| {
            let cells = claimed(group, level, missions);
            if number == 0 { cells.len() == 9 }
            else { LINES.iter().filter(|line| line.iter().all(|n| cells.contains(n))).count() >= number as usize }
        };
        if !achieved(before) && achieved(after) {
            result.extend(REWARDS.members().filter(|r| r["id"] == setting["masterBeginnerMissionRewardId"]).cloned());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_inventory_drives_training_bond_and_deck_goals() {
        let mut user = object! { card_list: [], character_list: [{exp: 30}], deck_list: [] };
        let master = super::super::custom_card::card_info(10010001);
        let exp = CARD_LEVELS.members().find(|l| l["id"] == master["masterCardLevelId"] && l["level"] == 30).unwrap()["exp"].clone();
        let curve = &databases::CARD_RARITY[master["rarity"].to_string()]["masterCardSkillLevelId"];
        let skill_exp = SKILL_LEVELS.members().find(|l| l["id"] == *curve && l["level"] == 2).unwrap()["exp"].clone();
        for id in 1..=5 {
            user["card_list"].push(object! {id: id, master_card_id: 10010001,
                exp: exp.clone(), skill_exp: skill_exp.clone(), evolve: [{type: 2, count: 1}]}).unwrap();
        }
        user["deck_list"].push(object! {main_card_ids: [1,2,3,4,5,0,0,0,0]}).unwrap();
        let mut missions = array![];
        let cleared = refresh(&user, &mut missions);
        for id in [1614001,1614002,1614003,1664001,1664002,1617001,1658001,1658002,1658003,1630001] {
            assert!(cleared.contains(id), "mission {}", id);
        }
        assert!(refresh(&user, &mut missions).is_empty());
        assert_eq!(items::get_mission_status(1605001, &missions)["progress"], 0);
    }

    #[test]
    fn counters_match_values_and_home_counts_only_unlocked_sheet() {
        let mut missions = array![];
        assert!(advance(63, Some(1110001), 20, &mut missions).is_empty());
        assert_eq!(advance(63, Some(8110001), 10, &mut missions), array![1663001]);
        assert_eq!(items::get_mission_status(1663002, &missions)["progress"], 10);
        assert_eq!(advance(63, Some(8110001), 10, &mut missions), array![1663002]);
        assert!(advance(6, Some(1), 1, &mut missions).is_empty());
        assert_eq!(advance(6, Some(2), 1, &mut missions), array![1606001]);
        advance(53, None, 3, &mut missions);
        assert_eq!(home_status(&missions), (2, false));
        for row in missions.members_mut() { row["status"] = 3.into(); }
        assert_eq!(home_status(&missions), (0, true));
        assert!(advance(53, None, 1, &mut missions).is_empty());
    }

    #[test]
    fn all_recorded_jp_beginner_claims_match() {
        let cases = jzon::parse(include_str!("beginner_mission_claims.json")).unwrap();
        assert_eq!(cases.len(), 15);
        for (i, case) in cases.members().enumerate() {
            let mut before = array![];
            ensure(&mut before);
            for row in before.members_mut() {
                if case["before"].contains(row["master_mission_id"].as_i64().unwrap()) { row["status"] = 3.into(); }
            }
            let mut after = before.clone();
            for row in after.members_mut() {
                if case["claim"].contains(row["master_mission_id"].as_i64().unwrap()) { row["status"] = 3.into(); }
            }
            let expected: Vec<_> = case["bonus"].members().map(|x| x.as_i64().unwrap()).collect();
            let actual: Vec<_> = new_rewards(&before, &after).iter().map(|x| x["amount"].as_i64().unwrap()).collect();
            assert_eq!(actual, expected, "JP claim {}", i);
        }
    }

    #[test]
    fn progress_is_capped_and_claims_are_not_reset() {
        let mut m = array![];
        advance(5, None, 12, &mut m);
        assert_eq!(items::get_mission_status(1605001, &m)["progress"], 1);
        assert_eq!(items::get_mission_status(1605002, &m)["progress"], 10);
        assert_eq!(items::get_mission_status(1605003, &m)["progress"], 12);
        assert!(!claimable(1605002, &m));
        for row in m.members_mut() { if row["master_mission_id"] == 1605001 { row["status"] = 3.into(); } }
        advance(5, None, 1, &mut m);
        assert_eq!(items::get_mission_status(1605001, &m)["status"], 3);
        for row in m.members_mut() {
            if row["master_mission_id"] == 1653001 {
                row["status"] = 2.into();
                row["progress"] = 50.into();
            }
        }
        ensure(&mut m);
        assert_eq!(items::get_mission_status(1653001, &m)["progress"], 1);
    }

    #[test]
    fn captured_first_sheet_claim_awards_one_line() {
        let mut before = array![];
        ensure(&mut before);
        let mut after = before.clone();
        for row in after.members_mut() {
            if [1653001,1605001,1663001,1666001,1658001].contains(&row["master_mission_id"].as_i64().unwrap()) {
                row["status"] = 3.into();
            }
        }
        let r = new_rewards(&before, &after);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["amount"], 50);
        assert!(new_rewards(&after, &after).is_empty());
    }

    #[test]
    fn full_sheet_gives_eight_lines_and_sheet_once() {
        let mut before = array![];
        ensure(&mut before);
        let mut after = before.clone();
        for row in after.members_mut() {
            if sheet_cells(1,1).any(|c| c["masterMissionId"] == row["master_mission_id"]) { row["status"] = 3.into(); }
        }
        let r = new_rewards(&before, &after);
        assert_eq!(r.len(), 9);
        assert_eq!(r.iter().map(|r| r["amount"].as_i64().unwrap()).sum::<i64>(), 1000);
        assert!(claimable(1605002, &after));
        assert!(!home_status(&after).1);
    }
}
