use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};

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
