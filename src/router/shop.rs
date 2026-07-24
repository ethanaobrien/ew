use jzon::{object};
use actix_web::{web, Responder};

use crate::router::{userdata, items, databases, Login, Session, Api};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/shop")
            .route("", web::get().to(shop))
            .route("/buy", web::post().to(buy))
    );
}

async fn shop(Login(key): Login) -> impl Responder {
    let user = userdata::get_acc(&key);

    Api(Some(object!{
        "shop_list": user["shop_list"].clone()
    }))
}

async fn buy(Session { key, body }: Session) -> impl Responder {
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

    Api(Some(object!{
        "gem": user["gem"].clone(),
        "shop_list": [bought],
        "gift_list": [],
        "updated_value_list": {
            "stamina": user["stamina"].clone()
        }
    }))
}
