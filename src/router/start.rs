use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

pub fn asset_hash(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["asset_version"].to_string() != global::ASSET_VERSION && body["asset_version"].to_string() != global::ASSET_VERSION_JP {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    let hash = if body["asset_version"].to_string() == global::ASSET_VERSION_JP {
        global::ASSET_HASH_JP
    } else {
        global::ASSET_HASH
    };
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "asset_hash": hash
        }
    };
    global::send(resp)
}

pub fn start(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["asset_version"].to_string() != global::ASSET_VERSION && body["asset_version"].to_string() != global::ASSET_VERSION_JP {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    let key = global::get_login(req.headers());
    let mut user = userdata::get_acc(&key);
    
    user["user"]["last_login_time"] = global::timestamp().into();
    user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    let hash = if body["asset_version"].to_string() == global::ASSET_VERSION_JP {
        global::ASSET_HASH_JP
    } else {
        global::ASSET_HASH
    };
    
    userdata::save_acc(&key, user);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "asset_hash": hash,
            "token": hex::encode("Hello") //what is this?
        }
    };
    global::send(resp)
}
