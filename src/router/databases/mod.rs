use json::{array, object, JsonValue};
use lazy_static::lazy_static;

use crate::include_file;

lazy_static! {
    pub static ref LOGIN_REWARDS: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/login_bonus_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref SHOP_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/shop_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref CHATS: JsonValue = {
        let mut chats = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/chat_room.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if chats[data["masterChatId"].to_string()].is_null() {
                chats[data["masterChatId"].to_string()] = object!{};
            }
            chats[data["masterChatId"].to_string()][data["roomId"].to_string()] = data.clone();
        }
        chats
    };
    pub static ref CHAPTERS: JsonValue = {
        let mut chats = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/chat_chapter.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if chats[data["masterChatId"].to_string()].is_null() {
                chats[data["masterChatId"].to_string()] = object!{};
            }
            chats[data["masterChatId"].to_string()][data["roomId"].to_string()] = data.clone();
        }
        chats
    };
    pub static ref EXCHANGE_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/exchange_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref EXCHANGE_REWARD: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/exchange_item_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref LIVE_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/live.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref MISSION_DATA: JsonValue = {
        json::parse(&include_file!("src/router/databases/json/live_mission.json")).unwrap()
    };
    pub static ref MISSION_COMBO_DATA: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/live_mission_combo.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["masterMusicId"].to_string()] = data.clone();
        }
        info
    };
    pub static ref MISSION_REWARD_DATA: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/live_mission_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref CARD_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/card.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref LOTTERY_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/login_bonus.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if info[data["id"].to_string()].is_null() {
                info[data["id"].to_string()] = object!{
                    info: data.clone(),
                    days: []
                };
            }
        }
        let days = json::parse(&include_file!("src/router/databases/json/login_bonus_reward_setting.json")).unwrap();
        for (_i, data) in days.members().enumerate() {
            if info[data["masterLoginBonusId"].to_string()].is_null() {
                continue;
            }
            info[data["masterLoginBonusId"].to_string()]["days"].push(data.clone()).unwrap();
        }
        let mut real_info = object!{};
        for (_i, data) in info.entries().enumerate() {
            real_info[data.1["info"]["id"].to_string()] = data.1.clone();
        }
        real_info
    };
    pub static ref CARDS: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
        }
        cardz
    };
    pub static ref POOL: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
            }
            cardz[data["id"].to_string()].push(data["number"].clone()).unwrap();
        }
        cardz
    };
    pub static ref RARITY: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery_rarity.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
            }
            cardz[data["id"].to_string()].push(data.clone()).unwrap();
        }
        cardz
    };
    pub static ref LOTTERY: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            cardz[data["id"].to_string()] = data.clone();
        }
        cardz
    };
    pub static ref PRICE: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery_price.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
        }
        cardz
    };
    pub static ref MISSION_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/mission.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref MISSION_REWARD: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/mission_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref ITEM_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref RANKS: JsonValue = {
        json::parse(&include_file!("src/router/databases/json/user_rank.json")).unwrap()
    };
}
