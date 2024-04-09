use json::{object, JsonValue};
use crate::encryption;
use actix_web::{
    HttpResponse,
    http::header::{HeaderValue, HeaderMap}
};
use crate::router::gree;
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{Engine as _, engine::general_purpose};

pub const ASSET_VERSION:          &str = "13177023d4b7ad41ff52af4cefba5c55";
pub const ASSET_HASH_ANDROID:     &str = "017ec1bcafbeea6a7714f0034b15bd0f";
pub const ASSET_HASH_IOS:         &str = "466d4616d14a8d8a842de06426e084c2";

pub const ASSET_VERSION_JP:       &str = "4c921d2443335e574a82e04ec9ea243c";
pub const ASSET_HASH_ANDROID_JP:  &str = "67f8f261c16b3cca63e520a25aad6c1c";
pub const ASSET_HASH_IOS_JP:      &str = "b8975be8300013a168d061d3fdcd4a16";


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

// true - added
// false - already has
pub fn give_character(id: String, user: &mut JsonValue) -> bool {
    for (_i, data) in user["card_list"].members().enumerate() {
        if data["master_card_id"].to_string() == id {
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

pub fn gift_item(item: &JsonValue, user: &mut JsonValue) {
    let to_push = object!{
        id: item["id"].clone(),
        reward_type: item["type"].clone(),
        give_type: item["giveType"].clone(),
        is_receive: 0,
        reason_text: "Because you logged in!!!!!!!!!!!!",
        value: item["value"].clone(),
        level: item["level"].clone(),
        amount: item["amount"].clone(),
        created_date_time: timestamp(),
        expire_date_time: timestamp() + (5 * (24 * 60 * 60)),
        received_date_time: 0
    };
    user["home"]["gift_list"].push(to_push).unwrap();
}
