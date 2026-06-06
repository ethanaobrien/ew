use jzon::{JsonValue, object};
use actix_web::HttpRequest;

use crate::encryption;
use crate::router::{userdata, global};

fn get_asset_hash(req: &HttpRequest, body: &JsonValue) -> String {
    if global::get_player_region(&body["asset_version"].to_string()).is_none() {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }

    let platform = req.headers()
        .get("aoharu-platform")
        .and_then(|v| v.to_str().ok())
        .map(global::parse_platform)
        .unwrap_or("Android");

    println!("Login on platform: {}", platform);

    global::get_asset_hash(&body["asset_version"].to_string(), platform).unwrap()
}

pub fn asset_hash(req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    Some(object!{
        "asset_hash": get_asset_hash(&req, &body)
    })
}

pub fn start(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    println!("Signin from uid: {}", user["user"]["id"].clone());
    
    user["user"]["last_login_time"] = global::timestamp().into();
    
    userdata::save_acc(&key, user);
    
    Some(object!{
        "asset_hash": get_asset_hash(&req, &body),
        "token": hex::encode("Hello") //what is this?
    })
}
