use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

pub fn asset_hash(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["asset_version"].to_string() != global::ASSET_VERSION {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "asset_hash": global::ASSET_HASH
        }
    };
    global::send(resp)
}

pub fn start(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["asset_version"].to_string() != global::ASSET_VERSION {
        println!("Warning! Asset version is not what was expected. (Did the app update?)");
    }
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let mut user = userdata::get_acc(key);
    
    user["user"]["last_login_time"] = global::timestamp().into();
    user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    userdata::save_acc(key, user);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "asset_hash": global::ASSET_HASH,
            "token": hex::encode("Hello") //what is this?
        }
    };
    global::send(resp)
}
