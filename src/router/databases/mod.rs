use json::{array, object, JsonValue};
use lazy_static::lazy_static;

use crate::include_file;

lazy_static! {
    pub static ref STORY: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/story_part.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref LOGIN_REWARDS: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/login_bonus_reward.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref SHOP_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/shop_item.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref CHATS: JsonValue = {
        let mut chats = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/chat_room.json")).unwrap();
        for data in items.members() {
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
        for data in items.members() {
            if chats[data["masterChatId"].to_string()].is_null() {
                chats[data["masterChatId"].to_string()] = object!{};
            }
            chats[data["masterChatId"].to_string()][data["roomId"].to_string()] = data.clone();
        }
        chats
    };
    pub static ref CHAPTERS_MASTER: JsonValue = {
        let mut chats = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/chat_chapter.json")).unwrap();
        for data in items.members() {
            chats[data["chapterId"].to_string()] = data.clone();
        }
        chats
    };
    pub static ref EXCHANGE_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/exchange_item.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref EXCHANGE_REWARD: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/exchange_item_reward.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref LIVE_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/live.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref LIVES: JsonValue = {
        json::parse(&include_file!("src/router/databases/json/live.json")).unwrap()
    };
    pub static ref MISSION_DATA: JsonValue = {
        json::parse(&include_file!("src/router/databases/json/live_mission.json")).unwrap()
    };
    pub static ref MISSION_COMBO_DATA: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/live_mission_combo.json")).unwrap();
        for data in items.members() {
            info[data["masterMusicId"].to_string()] = data.clone();
        }
        info
    };
    pub static ref MISSION_REWARD_DATA: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/live_mission_reward.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref CARD_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/card.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref LOTTERY_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/login_bonus.json")).unwrap();
        for data in items.members() {
            if info[data["id"].to_string()].is_null() {
                info[data["id"].to_string()] = object!{
                    info: data.clone(),
                    days: []
                };
            }
        }
        let days = json::parse(&include_file!("src/router/databases/json/login_bonus_reward_setting.json")).unwrap();
        for data in days.members() {
            if info[data["masterLoginBonusId"].to_string()].is_null() {
                continue;
            }
            info[data["masterLoginBonusId"].to_string()]["days"].push(data.clone()).unwrap();
        }
        let mut real_info = object!{};
        for data in info.entries() {
            real_info[data.1["info"]["id"].to_string()] = data.1.clone();
        }
        real_info
    };
    pub static ref CARDS: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery_item.json")).unwrap();
        let items_global = json::parse(&include_file!("src/router/databases/json/global/lottery_item.json")).unwrap();
        for data in items.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
        }
        for data in items_global.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            if cardz[data["id"].to_string()][data["number"].to_string()].is_null() {
                cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
            }
        }
        cardz
    };
    pub static ref POOL: JsonValue = {
        let mut cardz = object!{};
        let mut i2 = array![];
        let items = json::parse(&include_file!("src/router/databases/json/lottery_item.json")).unwrap();
        let items_global = json::parse(&include_file!("src/router/databases/json/global/lottery_item.json")).unwrap();
        for data in items.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
                i2.push(data["id"].to_string()).unwrap();
            }
            cardz[data["id"].to_string()].push(data["number"].clone()).unwrap();
        }
        for data in items_global.members() {
            if i2.contains(data["id"].to_string()) {
                continue;
            }
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
            }
            cardz[data["id"].to_string()].push(data["number"].clone()).unwrap();
        }
        cardz
    };
    pub static ref RARITY: JsonValue = {
        let mut cardz = object!{};
        let mut i2 = array![];
        let items = json::parse(&include_file!("src/router/databases/json/lottery_rarity.json")).unwrap();
        let items_global = json::parse(&include_file!("src/router/databases/json/global/lottery_rarity.json")).unwrap();
        for data in items.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
                i2.push(data["id"].to_string()).unwrap();
            }
            cardz[data["id"].to_string()].push(data.clone()).unwrap();
        }
        for data in items_global.members() {
            if i2.contains(data["id"].to_string()) {
                continue;
            }
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
        let items_global = json::parse(&include_file!("src/router/databases/json/global/lottery.json")).unwrap();
        for data in items.members() {
            cardz[data["id"].to_string()] = data.clone();
        }
        for data in items_global.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = data.clone();
            }
        }
        cardz
    };
    pub static ref PRICE: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/lottery_price.json")).unwrap();
        let items_global = json::parse(&include_file!("src/router/databases/json/global/lottery_price.json")).unwrap();
        for data in items.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
        }
        for data in items_global.members() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            if cardz[data["id"].to_string()][data["number"].to_string()].is_null() {
                cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
            }
        }
        cardz
    };
    pub static ref MISSION_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/mission.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref CHARACTER_CHATS: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/mission.json")).unwrap();
        for data in items.members() {
            if data["conditionValues"].len() != 1 || (data["conditionType"] != 50 && data["conditionType"] != 51) {
                continue;
            }
            if info[data["conditionValues"].to_string()].is_null() {
                info[data["conditionValues"].to_string()] = object!{};
            }
            info[data["conditionValues"][0].to_string()][data["conditionType"].to_string()] = array![data["masterMissionRewardId"].clone(), data["id"].clone()];
        }
        info
    };
    pub static ref MISSION_REWARD: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/mission_reward.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref ITEM_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/item.json")).unwrap();
        for data in items.members() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    pub static ref MUSIC: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/music.json")).unwrap();
        for live in LIVE_LIST.entries() {
            info[live.1["id"].to_string()] = loop {
                let mut val = object!{};
                for data in items.members() {
                    if live.1["masterMusicId"] == data["id"] {
                        val = data.clone();
                        break;
                    }
                }
                break val;
            };
        };
        info
    };
    pub static ref MUSIC_EN: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/global/music.json")).unwrap();
        for live in LIVE_LIST.entries() {
            info[live.1["id"].to_string()] = loop {
                let mut val = object!{};
                for data in items.members() {
                    if live.1["masterMusicId"] == data["id"] {
                        val = data.clone();
                        break;
                    }
                }
                break val;
            };
        };
        info
    };
    pub static ref RANKS: JsonValue = {
        json::parse(&include_file!("src/router/databases/json/user_rank.json")).unwrap()
    };
    pub static ref EVOLVE_COST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(&include_file!("src/router/databases/json/card_evolve.json")).unwrap();
        for data in items.members() {
            info[data["rarity"].to_string()] = data["price"].clone();
        }
        info
    };
}
