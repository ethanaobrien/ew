use json::{object, JsonValue};
use crate::encryption;
use actix_web::{
    HttpResponse,
    http::header::{HeaderValue, HeaderMap}
};
use std::time::{SystemTime, UNIX_EPOCH};

//different between ios and android?
pub const ASSET_VERSION: &str = "13177023d4b7ad41ff52af4cefba5c55";
pub const ASSET_HASH:    &str = "9fbfeda43a5cbf744ef23c06c22170aa";

pub const ASSET_VERSION_JP: &str = "4c921d2443335e574a82e04ec9ea243c";
pub const ASSET_HASH_JP:    &str = "67f8f261c16b3cca63e520a25aad6c1c";

pub fn get_login(headers: &HeaderMap) -> String {
    let blank_header = HeaderValue::from_static("");
    let key = headers.get("f19c72ba").unwrap_or(&blank_header).to_str().unwrap_or("");
    key.to_string()
}

pub fn timestamp() -> u64 {
    let now = SystemTime::now();

    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();
    return unix_timestamp.as_secs();
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
