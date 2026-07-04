use jzon::{object};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{userdata, global, items, databases};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/shop")
            .route("", web::get().to(shop))
            .route("/buy", web::post().to(buy))
    );
}

async fn shop(req: HttpRequest) -> impl Responder {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);

    global::api(&req, Some(object!{
        "shop_list": user["shop_list"].clone()
    }))
}

async fn buy(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);

    let shop_item_id = body["master_shop_item_id"].as_i64().unwrap();
    let item = &databases::SHOP_INFO[shop_item_id.to_string()];

    items::remove_gems(&mut user, item["price"].as_i64().unwrap());
    items::give_shop(shop_item_id, 1, &mut user);
    items::lp_modification(&mut user, item["price"].as_u64().unwrap() / 2, false);

    userdata::save_acc(&key, user.clone());

    let mut bought = object!{};
    for entry in user["shop_list"].members() {
        if entry["master_shop_item_id"].as_i64() == Some(shop_item_id) {
            bought = entry.clone();
            break;
        }
    }

    global::api(&req, Some(object!{
        "gem": user["gem"].clone(),
        "shop_list": [bought],
        "gift_list": [],
        "updated_value_list": {
            "stamina": user["stamina"].clone()
        }
    }))
}
