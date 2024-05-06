use json::{array, object, JsonValue};
use actix_web::{
    HttpResponse,
    http::header::{HeaderValue, HeaderMap},
    HttpRequest
};
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{Engine as _, engine::general_purpose};
use uuid::Uuid;

use crate::encryption;
use crate::router::{userdata, gree};

pub const ASSET_VERSION:          &str = "cb87bc1468c8631a262ff65b2960470b";
pub const ASSET_HASH_ANDROID:     &str = "8d1a9a5ce285c76b5afa2b5bdf154b9d";
pub const ASSET_HASH_IOS:         &str = "466d4616d14a8d8a842de06426e084c2";

pub const ASSET_VERSION_JP:       &str = "4c921d2443335e574a82e04ec9ea243c";
pub const ASSET_HASH_ANDROID_JP:  &str = "67f8f261c16b3cca63e520a25aad6c1c";
pub const ASSET_HASH_IOS_JP:      &str = "b8975be8300013a168d061d3fdcd4a16";

pub fn create_token() -> String {
    format!("{}", Uuid::now_v7())
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

pub fn timestamp_msec() -> u32 {
    let now = SystemTime::now();

    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();
    return unix_timestamp.subsec_nanos();
}

pub fn timestamp_since_midnight() -> u64 {
    let now = SystemTime::now();
    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();

    let midnight = unix_timestamp.as_secs() % (24 * 60 * 60);

    let rv = unix_timestamp.as_secs() - midnight;
    rv
}

fn init_time(server_data: &mut JsonValue, token: &str) {
    let mut edited = false;
    if server_data["server_time_set"].as_u64().is_none() {
        server_data["server_time_set"] = timestamp().into();
        edited = true;
    }
    if server_data["server_time"].as_u64().is_none() {
        server_data["server_time"] = 1709272800.into();
        edited = true;
    }
    if edited {
        userdata::save_server_data(&token, server_data.clone());
    }
}

fn set_time(data: &mut JsonValue, req: HttpRequest) {
    let blank_header = HeaderValue::from_static("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("").parse::<i64>().unwrap_or(0);
    if uid == 0 {
        return;
    }
    let token = userdata::get_login_token(uid);
    let mut server_data = userdata::get_server_data(&token);
    init_time(&mut server_data, &token);
    
    let time_set = server_data["server_time_set"].as_u64().unwrap_or(timestamp());
    let server_time = server_data["server_time"].as_u64().unwrap_or(0);//1711741114
    if server_time == 0 {
        return;
    }
    
    let time_since_set = timestamp() - time_set;
    data["server_time"] = (server_time + time_since_set).into();
}

pub fn send(mut data: JsonValue, req: HttpRequest) -> HttpResponse {
    //println!("{}", json::stringify(data.clone()));
    set_time(&mut data, req);
    
    let encrypted = encryption::encrypt_packet(&json::stringify(data)).unwrap();
    let resp = encrypted.into_bytes();

    HttpResponse::Ok().body(resp)
}

pub fn error_resp(req: HttpRequest) -> HttpResponse {
    send(object!{}, req)
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

pub fn get_card(id: i64, user: &JsonValue) -> JsonValue {
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

fn get_clear_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for (_i, current) in user["live_list"].members().enumerate() {
        if current["level"].to_string() == level.to_string() {
            rv += 1;
        }
    }
    rv
}

fn get_full_combo_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for (_i, current) in user["live_mission_list"].members().enumerate() {
        if current["clear_master_live_mission_ids"].contains(20 + level) {
            rv += 1;
        }
    }
    rv
}

fn get_perfect_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for (_i, current) in user["live_mission_list"].members().enumerate() {
        if current["clear_master_live_mission_ids"].contains(40 + level) {
            rv += 1;
        }
    }
    rv
}

pub fn get_user(id: i64, friends: &JsonValue, live_data: bool) -> JsonValue {
    let user = userdata::get_acc_from_uid(id);
    if !user["error"].is_empty() {
        return object!{};
    }
    
    let mut rv = object!{
        user: user["user"].clone(),
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
    if live_data {
        rv["live_data_summary"] = object!{
            clear_count_list: [get_clear_count(&user, 1), get_clear_count(&user, 2), get_clear_count(&user, 3), get_clear_count(&user, 4)],
            full_combo_list: [get_full_combo_count(&user, 1), get_full_combo_count(&user, 2), get_full_combo_count(&user, 3), get_full_combo_count(&user, 4)],
            all_perfect_list: [get_perfect_count(&user, 1), get_perfect_count(&user, 2), get_perfect_count(&user, 3), get_perfect_count(&user, 4)],
            high_score_rate: {
                rate: 0,
                detail: []
            }
        };
    }
    rv["user"].remove("sif_user_id");
    rv["user"].remove("ss_user_id");
    rv["user"].remove("birthday");
    
    if !friends.is_empty() {
        rv["status"] = if friends["friend_user_id_list"].contains(id) {
            3
        } else if friends["pending_user_id_list"].contains(id) {
            2
        } else if friends["request_user_id_list"].contains(id) {
            1
        } else {
            0
        }.into();
    }
    
    rv
}
