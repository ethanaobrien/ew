use jzon::{JsonValue, object};
use actix_web::{web, HttpRequest, Responder};

use crate::encryption;
use crate::router::{userdata, global};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/start", web::post().to(start));
    cfg.route("/start/assetHash", web::post().to(asset_hash));
}

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

async fn asset_hash(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();

    global::api(&req, Some(object!{
        "asset_hash": get_asset_hash(&req, &body)
    }))
}

async fn start(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    println!("Signin from uid: {}", user["user"]["id"].clone());
    
    user["user"]["last_login_time"] = global::timestamp().into();
    
    userdata::save_acc(&key, user);
    
    global::api(&req, Some(object!{
        "asset_hash": get_asset_hash(&req, &body),
        "token": hex::encode("Hello") //what is this?
    }))
}
