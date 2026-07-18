use jzon::{JsonValue, object};
use actix_web::{web, HttpRequest, Responder};

use crate::encryption;
use crate::router::{userdata, global};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/start", web::post().to(start));
    cfg.route("/start/assetHash", web::post().to(asset_hash));
}

fn get_asset_hash(req: &HttpRequest, body: &JsonValue) -> Option<String> {
    let platform = req.headers()
        .get("aoharu-platform")
        .and_then(|v| v.to_str().ok())
        .map(global::parse_platform)
        .unwrap_or("Android");

    println!("Login on platform: {}", platform);

    let rv = global::get_asset_hash(&body["asset_version"].to_string(), platform);
    if rv.is_none() {
        println!("Unknown asset version {}; telling the client to update.", body["asset_version"]);
    }
    rv
}

async fn asset_hash(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();

    match get_asset_hash(&req, &body) {
        Some(hash) => global::api(&req, Some(object!{
            "asset_hash": hash
        })),
        None => global::api_error(&req, global::RESULT_GAME_VERSION_UPDATED),
    }
}

async fn start(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();

    let Some(asset_hash) = get_asset_hash(&req, &body) else {
        return global::api_error(&req, global::RESULT_GAME_VERSION_UPDATED);
    };

    let mut user = userdata::get_acc(&key);

    println!("Signin from uid: {}", user["user"]["id"].clone());

    user["user"]["last_login_time"] = global::timestamp().into();

    userdata::save_acc(&key, user);

    global::api(&req, Some(object!{
        "asset_hash": asset_hash,
        "token": hex::encode("Hello") //what is this?
    }))
}
