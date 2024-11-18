use json::{JsonValue, object};
use actix_web::{HttpRequest, http::header::HeaderValue};

use crate::encryption;
use crate::router::{userdata, global};

fn get_asset_hash(req: &HttpRequest, body: &JsonValue) -> String {
    if body["asset_version"] != global::ASSET_VERSION && body["asset_version"] != global::ASSET_VERSION_JP {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    
    let blank_header = HeaderValue::from_static("");
    let platform = req.headers().get("aoharu-platform").unwrap_or(&blank_header).to_str().unwrap_or("");
    let android = !platform.to_lowercase().contains("iphone");
    
    global::get_asset_hash(body["asset_version"].to_string(), android)
}

pub fn asset_hash(req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    Some(object!{
        "asset_hash": get_asset_hash(&req, &body)
    })
}

pub fn start(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    println!("Signin from uid: {}", user["user"]["id"].clone());
    
    user["user"]["last_login_time"] = global::timestamp().into();
    
    userdata::save_acc(&key, user);
    
    Some(object!{
        "asset_hash": get_asset_hash(&req, &body),
        "token": hex::encode("Hello") //what is this?
    })
}
