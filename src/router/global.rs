use json::{object, JsonValue, array};
use crate::encryption;
use actix_web::{
    HttpResponse,
    http::header::{HeaderValue, HeaderMap}
};
use crate::router::gree;
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{Engine as _, engine::general_purpose};
use crate::router::userdata;
use lazy_static::lazy_static;

pub const ASSET_VERSION:          &str = "13177023d4b7ad41ff52af4cefba5c55";
pub const ASSET_HASH_ANDROID:     &str = "017ec1bcafbeea6a7714f0034b15bd0f";
pub const ASSET_HASH_IOS:         &str = "466d4616d14a8d8a842de06426e084c2";

pub const ASSET_VERSION_JP:       &str = "4c921d2443335e574a82e04ec9ea243c";
pub const ASSET_HASH_ANDROID_JP:  &str = "67f8f261c16b3cca63e520a25aad6c1c";
pub const ASSET_HASH_IOS_JP:      &str = "b8975be8300013a168d061d3fdcd4a16";

lazy_static! {
    static ref ITEM_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
}

pub fn get_item_info(id: i64) -> JsonValue {
    ITEM_INFO[id.to_string()].clone()
}

pub fn remove_gems(user: &mut JsonValue, amount: i64) {
    let mut amount = amount;
    let mut free = user["gem"]["free"].as_i64().unwrap();
    let mut paid = user["gem"]["charge"].as_i64().unwrap();
    
    free -= amount;
    if free < 0 {
        amount = -free;
        free = 0;
    }
    paid -= amount;
    if paid < 0 {
        paid = 0;
    }
    user["gem"]["free"] = free.into();
    user["gem"]["charge"] = paid.into();
    user["gem"]["total"] = (free + paid).into();
}

fn get_uuid(input: &str) -> Option<String> {
    let key = "sk1bdzb310n0s9tl";
    let key_index = match input.find(key) {
        Some(index) => index + key.len(),
        None => return None,
    };
    let after = &input[key_index..];

    let uuid_length = 36;
    if after.len() >= uuid_length {
        let uuid = &after[..uuid_length];
        return Some(uuid.to_string());
    }

    None
}
pub fn get_login(headers: &HeaderMap, body: &str) -> String {
    let blank_header = HeaderValue::from_static("");
    
    let login = headers.get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let decoded = general_purpose::STANDARD.decode(login).unwrap_or(vec![]);
    match get_uuid(&String::from_utf8_lossy(&decoded).to_string()) {
        Some(token) => {
            return token;
        },
        None => {
            let rv = gree::get_uuid(headers, body);
            assert!(rv != String::new());
            return rv;
        },
    };
}

pub fn timestamp() -> u64 {
    let now = SystemTime::now();

    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();
    return unix_timestamp.as_secs();
}
pub fn timestamp_since_midnight() -> u64 {
    let now = SystemTime::now();
    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();

    let midnight = unix_timestamp.as_secs() % (24 * 60 * 60);

    let rv = unix_timestamp.as_secs() - midnight;
    rv
}

pub fn send(data: JsonValue) -> HttpResponse {
    //println!("{}", json::stringify(data.clone()));
    let encrypted = encryption::encrypt_packet(&json::stringify(data)).unwrap();
    let resp = encrypted.into_bytes();

    HttpResponse::Ok().body(resp)
}

pub fn error_resp() -> HttpResponse {
    send(object!{})
}


// true - limit reached
// false - all good
const GIFT_LIMIT: usize = 100000;
const LIMIT_ITEMS: i64 = 200000000;
const LIMIT_COINS: i64 = 2000000000;
const LIMIT_PRIMOGEMS: i64 = 2000000000;

pub fn give_item(master_item_id: i64, amount: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for (_j, dataa) in user["item_list"].members_mut().enumerate() {
        if dataa["master_item_id"].as_i64().unwrap() == master_item_id {
            has = true;
            let new_amount = dataa["amount"].as_i64().unwrap() + amount;
            if new_amount > LIMIT_ITEMS {
                return true;
            }
            dataa["amount"] = new_amount.into();
            break;
        }
    }
    if !has {
        user["item_list"].push(object!{
            id: master_item_id,
            master_item_id: master_item_id,
            amount: amount,
            expire_date_time: null
        }).unwrap();
    }
    false
}

pub fn give_points(master_item_id: i64, amount: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for (_j, dataa) in user["point_list"].members_mut().enumerate() {
        if dataa["type"].as_i64().unwrap() == master_item_id {
            has = true;
            let new_amount = dataa["amount"].as_i64().unwrap() + amount;
            if new_amount > LIMIT_COINS {
                return true;
            }
            dataa["amount"] = new_amount.into();
            break;
        }
    }
    if !has {
        user["point_list"].push(object!{
            type: master_item_id,
            amount: amount
        }).unwrap();
    }
    false
}

pub fn start_login_bonus(id: i64, bonus: &mut JsonValue) -> bool {
    if crate::router::login::get_login_bonus_info(id).is_empty() {
        return false;
    }
    for (_j, dataa) in bonus["bonus_list"].members().enumerate() {
        if dataa["master_login_bonus_id"].as_i64().unwrap() == id {
            return false;
        }
    }
    bonus["bonus_list"].push(object!{
        master_login_bonus_id: id,
        day_counts: [],
        event_bonus_list: []
    }).unwrap();
    true
}

pub fn give_primogems(amount: i64, user: &mut JsonValue) -> bool {
    let new_amount = user["gem"]["free"].as_i64().unwrap() + amount;
    if new_amount > LIMIT_PRIMOGEMS {
        return true;
    }
    
    user["gem"]["free"] = new_amount.into();
    false
}

pub fn gift_item(item: &JsonValue, reason: &str, user: &mut JsonValue) {
    let to_push = object!{
        id: item["id"].clone(),
        reward_type: item["type"].clone(),
        is_receive: 0,
        reason_text: reason,
        value: item["value"].clone(),
        level: item["level"].clone(),
        amount: item["amount"].clone(),
        created_date_time: timestamp(),
        expire_date_time: timestamp() + (5 * (24 * 60 * 60)),
        received_date_time: 0
    };
    if user["home"]["gift_list"].len() >= GIFT_LIMIT {
        return;
    }
    user["home"]["gift_list"].push(to_push).unwrap();
}


// true - added
// false - already has
pub fn give_character(id: String, user: &mut JsonValue) -> bool {
    for (_i, data) in user["card_list"].members().enumerate() {
        if data["master_card_id"].to_string() == id {
            give_item(19100001, 100, user);
            return false;
        }
    }
    
    let to_push = object!{
        "id": id.parse::<i32>().unwrap(),
        "master_card_id": id.parse::<i32>().unwrap(),
        "exp": 0,
        "skill_exp": 0,
        "evolve": [],
        "created_date_time": timestamp()
    };
    user["card_list"].push(to_push.clone()).unwrap();
    true
}

pub fn get_user_rank_data(exp: i64) -> JsonValue {
    let ranks = json::parse(include_str!("userdata/user_rank.json")).unwrap();
    
    for (i, rank) in ranks.members().enumerate() {
        if exp < rank["exp"].as_i64().unwrap() {
            return ranks[i - 1].clone();
        }
    }
    return ranks[ranks.len() - 1].clone();
}

pub fn give_exp(amount: i32, user: &mut JsonValue) {
    let current_rank = get_user_rank_data(user["user"]["exp"].as_i64().unwrap());
    user["user"]["exp"] = (user["user"]["exp"].as_i32().unwrap() + amount).into();
    let new_rank = get_user_rank_data(user["user"]["exp"].as_i64().unwrap());
    if current_rank["rank"].to_string() != new_rank["rank"].to_string() {
        user["stamina"]["stamina"] = (user["stamina"]["stamina"].as_i64().unwrap() + new_rank["maxLp"].as_i64().unwrap()).into();
        user["stamina"]["last_updated_time"] = timestamp().into();
    }
}

fn get_card(id: i64, user: &JsonValue) -> JsonValue {
    if id == 0 {
        return object!{};
    }
    
    for (_i, data) in user["card_list"].members().enumerate() {
        if data["master_card_id"].as_i64().unwrap_or(0) == id {
            return data.clone();
        }
    }
    return object!{};
}
fn get_cards(arr: JsonValue, user: &JsonValue) -> JsonValue {
    let mut rv = array![];
    for (_i, data) in arr.members().enumerate() {
        let to_push = get_card(data.as_i64().unwrap_or(0), user);
        if to_push.is_empty() {
            continue;
        }
        rv.push(to_push).unwrap();
    }
    return rv;
}
pub fn get_user(id: i64, friends: &JsonValue) -> JsonValue {
    let user = userdata::get_acc_from_uid(id);
    if !user["error"].is_empty() {
        return object!{};
    }
    let mut rv = object!{
        user: user["user"].clone(),
        live_data_summary: {
            clear_count_list: [0, 0, 0, 0],
            full_combo_list: [0, 0, 0, 0],
            all_perfect_list: [0, 0, 0, 0],
            high_score_rate: {
                rate: 0,
                detail: []
            }
        },
        main_deck_detail: {
            total_power: 0, //how to calculate?
            deck: user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1].clone(),
            card_list: get_cards(user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1]["main_card_ids"].clone(), &user)
        },
        favorite_card: get_card(user["user"]["favorite_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_smile_card: get_card(user["user"]["guest_smile_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_cool_card: get_card(user["user"]["guest_cool_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_pure_card: get_card(user["user"]["guest_pure_master_card_id"].as_i64().unwrap_or(0), &user),
        master_title_ids: user["user"]["master_title_ids"].clone()
    };
    rv["user"].remove("sif_user_id");
    rv["user"].remove("ss_user_id");
    rv["user"].remove("birthday");
    
    rv["status"] = if friends["friend_user_id_list"].contains(id) {
        3
    } else if friends["pending_user_id_list"].contains(id) {
        2
    } else if friends["request_user_id_list"].contains(id) {
        1
    } else {
        0
    }.into();
    
    rv
}
