use jzon::{array, object, JsonValue};
use lazy_static::lazy_static;

pub mod csv;

use csv::Region;

fn t(name: &str) -> JsonValue { csv::table(Region::Jp, name) }
fn g(name: &str) -> JsonValue { csv::table(Region::En, name) }

fn index_by(items: &JsonValue, key: &str) -> JsonValue {
    let mut info = object! {};
    for data in items.members() {
        info[data[key].to_string()] = data.clone();
    }
    info
}

// Missions in an id band grouped by the character they target, as
// [requiredBond, missionId] ordered by required bond. Read from masterdata
// rather than derived from the character id: the ids do not follow character
// order (4010/4011 sit at 1158079/1158080, after the 15000 tier), and
// characters added by a card import have none at all.
fn missions_by_character(lo: i64, hi: i64) -> JsonValue {
    let mut acc: std::collections::HashMap<String, Vec<(i64, i64)>> = std::collections::HashMap::new();
    for data in t("mission").members() {
        let id = data["id"].as_i64().unwrap_or(0);
        if id < lo || id >= hi || data["conditionValues"].len() != 1 {
            continue;
        }
        acc.entry(data["conditionValues"][0].to_string())
            .or_default()
            .push((data["conditionNumber"].as_i64().unwrap_or(0), id));
    }
    let mut info = object! {};
    for (character, mut list) in acc {
        list.sort();
        let mut entries = array![];
        for (required, id) in list {
            entries.push(array![required, id]).unwrap();
        }
        info[&character] = entries;
    }
    info
}

lazy_static! {
    pub static ref STORY: JsonValue = index_by(&t("story_part"), "id");

    pub static ref LOGIN_REWARDS: JsonValue = index_by(&t("login_bonus_reward"), "id");

    pub static ref SHOP_INFO: JsonValue = index_by(&t("shop_item"), "id");

    pub static ref CHATS: JsonValue = {
        let mut chats = object! {};
        for data in t("chat_room").members() {
            let chat_id = data["masterChatId"].to_string();
            if chats[&chat_id].is_null() {
                chats[&chat_id] = object! {};
            }
            chats[&chat_id][data["roomId"].to_string()] = data.clone();
        }
        chats
    };

    pub static ref CHAPTERS: JsonValue = {
        let mut chats = object! {};
        for data in t("chat_chapter").members() {
            let chat_id = data["masterChatId"].to_string();
            if chats[&chat_id].is_null() {
                chats[&chat_id] = object! {};
            }
            chats[&chat_id][data["roomId"].to_string()] = data.clone();
        }
        chats
    };

    pub static ref CHAPTERS_MASTER: JsonValue = index_by(&t("chat_chapter"), "chapterId");

    pub static ref EXCHANGE_LIST: JsonValue = index_by(&t("exchange_item"), "id");

    pub static ref EXCHANGE_REWARD: JsonValue = index_by(&t("exchange_item_reward"), "id");

    pub static ref LIVE_LIST: JsonValue = index_by(&t("live"), "id");

    pub static ref CLEAR_REWARD: JsonValue = t("live_clear_reward");

    pub static ref LIVES: JsonValue = t("live");

    pub static ref MISSION_DATA: JsonValue = t("live_mission");

    pub static ref MISSION_COMBO_DATA: JsonValue =
        index_by(&t("live_mission_combo"), "masterMusicId");

    pub static ref MISSION_REWARD_DATA: JsonValue =
        index_by(&t("live_mission_reward"), "id");

    pub static ref CARD_LIST: JsonValue = index_by(&t("card"), "id");

    pub static ref LOTTERY_INFO: JsonValue = {
        let mut info = object! {};
        for data in t("login_bonus").members() {
            let id = data["id"].to_string();
            if info[&id].is_null() {
                info[&id] = object! { info: data.clone(), days: [] };
            }
        }
        for data in t("login_bonus_reward_setting").members() {
            let id = data["masterLoginBonusId"].to_string();
            if info[&id].is_null() {
                continue;
            }
            info[&id]["days"].push(data.clone()).unwrap();
        }
        let mut real_info = object! {};
        for entry in info.entries() {
            real_info[entry.1["info"]["id"].to_string()] = entry.1.clone();
        }
        real_info
    };

    pub static ref CARDS: JsonValue = {
        let mut cardz = object! {};
        for data in t("lottery_item").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() { cardz[&id] = object! {}; }
            cardz[&id][data["number"].to_string()] = data.clone();
        }
        for data in g("lottery_item").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() { cardz[&id] = object! {}; }
            let num = data["number"].to_string();
            if cardz[&id][&num].is_null() {
                cardz[&id][&num] = data.clone();
            }
        }
        cardz
    };

    pub static ref POOL: JsonValue = {
        let mut cardz = object! {};
        let mut seen_ids = array![];
        for data in t("lottery_item").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() {
                cardz[&id] = array![];
                seen_ids.push(id.clone()).unwrap();
            }
            cardz[&id].push(data["number"].clone()).unwrap();
        }
        for data in g("lottery_item").members() {
            let id = data["id"].to_string();
            if seen_ids.contains(id.as_str()) { continue; }
            if cardz[&id].is_null() { cardz[&id] = array![]; }
            cardz[&id].push(data["number"].clone()).unwrap();
        }
        cardz
    };

    pub static ref RARITY: JsonValue = {
        let mut cardz = object! {};
        let mut seen_ids = array![];
        for data in t("lottery_rarity").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() {
                cardz[&id] = array![];
                seen_ids.push(id.clone()).unwrap();
            }
            cardz[&id].push(data.clone()).unwrap();
        }
        for data in g("lottery_rarity").members() {
            let id = data["id"].to_string();
            if seen_ids.contains(id.as_str()) { continue; }
            if cardz[&id].is_null() { cardz[&id] = array![]; }
            cardz[&id].push(data.clone()).unwrap();
        }
        cardz
    };

    pub static ref STEPUP: JsonValue = {
        let mut cardz = object! {};
        let mut seen_ids = array![];
        for data in t("lottery_stepup").members() {
            let id = data["masterLotteryId"].to_string();
            if cardz[&id].is_null() {
                cardz[&id] = array![];
                seen_ids.push(id.clone()).unwrap();
            }
            cardz[&id].push(data.clone()).unwrap();
        }
        for data in g("lottery_stepup").members() {
            let id = data["masterLotteryId"].to_string();
            if seen_ids.contains(id.as_str()) { continue; }
            if cardz[&id].is_null() { cardz[&id] = array![]; }
            cardz[&id].push(data.clone()).unwrap();
        }
        cardz
    };

    pub static ref LOTTERY: JsonValue = {
        let mut cardz = object! {};
        for data in t("lottery").members() {
            cardz[data["id"].to_string()] = data.clone();
        }
        for data in g("lottery").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() {
                cardz[&id] = data.clone();
            }
        }
        cardz
    };

    pub static ref PRICE: JsonValue = {
        let mut cardz = object! {};
        for data in t("lottery_price").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() { cardz[&id] = object! {}; }
            cardz[&id][data["number"].to_string()] = data.clone();
        }
        for data in g("lottery_price").members() {
            let id = data["id"].to_string();
            if cardz[&id].is_null() { cardz[&id] = object! {}; }
            let num = data["number"].to_string();
            if cardz[&id][&num].is_null() {
                cardz[&id][&num] = data.clone();
            }
        }
        cardz
    };

    pub static ref MISSION_LIST: JsonValue = index_by(&t("mission"), "id");

    pub static ref CHARACTER_BOND_MISSIONS: JsonValue = missions_by_character(1158000, 1159000);

    // 15 per character, aligned with live::CHATS.
    pub static ref CHARACTER_CHAT_MISSIONS: JsonValue = missions_by_character(1958000, 1960000);

    pub static ref CHARACTER_CHATS: JsonValue = {
        let mut info = object! {};
        for data in t("mission").members() {
            if data["conditionValues"].len() != 1
                || (data["conditionType"] != 50 && data["conditionType"] != 51)
            {
                continue;
            }
            let cv0 = data["conditionValues"][0].to_string();
            if info[&cv0].is_null() { info[&cv0] = object! {}; }
            info[&cv0][data["conditionType"].to_string()] = array![
                data["masterMissionRewardId"].clone(),
                data["id"].clone()
            ];
        }
        info
    };

    pub static ref MISSION_REWARD: JsonValue = index_by(&t("mission_reward"), "id");

    pub static ref MISSION_REWARDS: JsonValue = {
        let mut info = object! {};
        for data in t("mission_reward").members() {
            let id = data["id"].to_string();
            if info[&id].is_null() {
                info[&id] = array![];
            }
            info[&id].push(data.clone()).unwrap();
        }
        info
    };

    pub static ref ITEM_INFO: JsonValue = index_by(&t("item"), "id");

    pub static ref MUSIC: JsonValue = {
        let music = t("music");
        let mut info = object! {};
        for live in LIVE_LIST.entries() {
            let mut val = object! {};
            for data in music.members() {
                if live.1["masterMusicId"] == data["id"] {
                    val = data.clone();
                    break;
                }
            }
            info[live.1["id"].to_string()] = val;
        }
        info
    };

    pub static ref MUSIC_EN: JsonValue = {
        let music = g("music");
        let mut info = object! {};
        for live in LIVE_LIST.entries() {
            let mut val = object! {};
            for data in music.members() {
                if live.1["masterMusicId"] == data["id"] {
                    val = data.clone();
                    break;
                }
            }
            info[live.1["id"].to_string()] = val;
        }
        info
    };

    // const.csv keyed by _id. Values are strings in masterdata, exactly as the
    // client reads them (ConstMst._value + StringExtensions.ToIntOrDefault).
    pub static ref CONST: JsonValue = index_by(&t("const"), "id");

    pub static ref LIVE_BOOST: JsonValue = index_by(&t("live_boost"), "value");

    // The stamps every account starts with (chat_stamp._initialStamp), in masterdata
    // order. Officially this is the whole of a fresh account's master_chat_stamp_ids —
    // captured /api/chat/home responses open with exactly this list before an account's
    // earned stamps are appended (see chat::tests::the_initial_stamp_set_matches_official).
    pub static ref INITIAL_CHAT_STAMPS: JsonValue = {
        let mut ids = array![];
        for data in t("chat_stamp").members() {
            if data["initialStamp"].as_i64().unwrap_or(0) == 1 {
                ids.push(data["id"].clone()).unwrap();
            }
        }
        ids
    };

    pub static ref EVENTS: JsonValue = index_by(&t("event"), "id");

    // release_label.csv keyed by _id — the open/close window masterdata rows are gated
    // on. _openedAt / _closedAt are blank for the evergreen label (id 1).
    pub static ref RELEASE_LABEL: JsonValue = index_by(&t("release_label"), "id");

    // event_score.csv keyed by _masterEventId (Shock.EventScoreMst) — the per-event
    // event-point yield of one live. Ratios are 1/10000, like every other ratio the
    // client divides by COMMON_CONST.RATIO_DIVISOR.
    pub static ref EVENT_SCORE: JsonValue = index_by(&t("event_score"), "masterEventId");

    // music_level rows keyed "{masterMusicId}_{level}" — _fullCombo is the note
    // count the multi-live miss/great-perfect ratios are measured against.
    pub static ref MUSIC_LEVEL: JsonValue = {
        let mut info = object! {};
        for data in t("music_level").members() {
            info[format!("{}_{}", data["masterMusicId"], data["level"])] = data.clone();
        }
        info
    };

    // multievent_rankbonus keyed "{playerCount}_{liveRank}" — _eventPtBonus is a
    // ratio in 1/10000 (the client renders these as `sum / 100` percent).
    pub static ref MULTIEVENT_RANK_BONUS: JsonValue = {
        let mut info = object! {};
        for data in t("multievent_rankbonus").members() {
            info[format!("{}_{}", data["playerCount"], data["liveRank"])] = data.clone();
        }
        info
    };

    pub static ref RANKS: JsonValue = t("user_rank");

    pub static ref USER_RANK_REWARD: JsonValue = {
        let mut info = object! {};
        for data in t("user_rank_reward").members() {
            let id = data["id"].to_string();
            if info[&id].is_null() {
                info[&id] = array![];
            }
            info[&id].push(data.clone()).unwrap();
        }
        info
    };

    pub static ref EVOLVE_COST: JsonValue = {
        let mut info = object! {};
        for data in t("card_evolve").members() {
            info[data["rarity"].to_string()] = data["price"].clone();
        }
        info
    };

    pub static ref CARD_RARITY: JsonValue = index_by(&t("card_rarity"), "rarity");

    pub static ref CARD_EVOLVE: JsonValue = index_by(&t("card_evolve"), "rarity");

    pub static ref CARD_LEVEL: JsonValue = {
        let mut info = object! {};
        for data in t("card_level").members() {
            info[format!("{}_{}", data["id"], data["level"])] = data["exp"].clone();
        }
        info
    };

    pub static ref CARD_SKILL_MAX: JsonValue = {
        let mut info = object! {};
        for data in t("card_skill_level").members() {
            let id = data["id"].to_string();
            let exp = data["exp"].as_i64().unwrap_or(0);
            if exp > info[&id].as_i64().unwrap_or(0) {
                info[id] = exp.into();
            }
        }
        info
    };
}
