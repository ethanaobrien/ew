use json::JsonValue;
use crate::encryption;
use actix_web::{
    HttpResponse
};
use std::time::{SystemTime, UNIX_EPOCH};

//likely different between ios and android?
pub const ASSET_VERSION: &str = "4a802a747076a91e5e62707f6358bc2d";
pub const ASSET_HASH:    &str = "183931205c9dbc39788ef7b361988cf4";

pub fn timestamp() -> u64 {
    let now = SystemTime::now();

    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();
    return unix_timestamp.as_secs();
}

pub fn send(data: JsonValue) -> HttpResponse {
    let encrypted = encryption::encrypt_packet(&json::stringify(data)).unwrap();
    let resp = encrypted.into_bytes();

    HttpResponse::Ok().body(resp)
}