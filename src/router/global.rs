use json::{object, JsonValue};
use crate::encryption;
use actix_web::{
    HttpResponse
};
use std::time::{SystemTime, UNIX_EPOCH};

//likely different between ios and android?
pub const ASSET_VERSION: &str = "4a802a747076a91e5e62707f6358bc2d";
pub const ASSET_HASH:    &str = "0de9f85900e910b0b4873dcdd0933aa5";

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
        "id": id.clone(),
        "master_card_id": id,
        "exp": 0,
        "skill_exp": 0,
        "evolve": [],
        "created_date_time": timestamp()
    };
    user["card_list"].push(to_push.clone()).unwrap();
    true
}
