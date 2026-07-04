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

    pub static ref RANKS: JsonValue = t("user_rank");

    pub static ref EVOLVE_COST: JsonValue = {
        let mut info = object! {};
        for data in t("card_evolve").members() {
            info[data["rarity"].to_string()] = data["price"].clone();
        }
        info
    };
}
