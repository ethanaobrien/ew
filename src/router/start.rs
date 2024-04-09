use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

pub fn asset_hash(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["asset_version"].to_string() != global::ASSET_VERSION && body["asset_version"].to_string() != global::ASSET_VERSION_JP {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    
    let blank_header = HeaderValue::from_static("");
    let platform = req.headers().get("aoharu-platform").unwrap_or(&blank_header).to_str().unwrap_or("");
    let android = !platform.to_lowercase().contains("iphone");
    
    let hash = if body["asset_version"].to_string() == global::ASSET_VERSION_JP {
        if android {
            global::ASSET_HASH_ANDROID_JP
        } else {
            global::ASSET_HASH_IOS_JP
        }
    } else {
        if android {
            global::ASSET_HASH_ANDROID
        } else {
            global::ASSET_HASH_IOS
        }
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
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["asset_version"].to_string() != global::ASSET_VERSION && body["asset_version"].to_string() != global::ASSET_VERSION_JP {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    let mut user = userdata::get_acc(&key);
    
    println!("Signin from uid: {}", user["user"]["id"].clone());
    
    user["user"]["last_login_time"] = global::timestamp().into();
    user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    let blank_header = HeaderValue::from_static("");
    let platform = req.headers().get("aoharu-platform").unwrap_or(&blank_header).to_str().unwrap_or("");
    let android = !platform.to_lowercase().contains("iphone");
    
    let hash = if body["asset_version"].to_string() == global::ASSET_VERSION_JP {
        if android {
            global::ASSET_HASH_ANDROID_JP
        } else {
            global::ASSET_HASH_IOS_JP
        }
    } else {
        if android {
            global::ASSET_HASH_ANDROID
        } else {
            global::ASSET_HASH_IOS
        }
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
