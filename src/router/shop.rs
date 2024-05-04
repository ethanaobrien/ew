use json::{object, JsonValue};
use actix_web::{HttpResponse, HttpRequest};
use lazy_static::lazy_static;

use crate::router::{userdata, global, items};
use crate::encryption;

lazy_static! {
    static ref SHOP_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/shop_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
}

fn get_item_info(id: i64) -> JsonValue {
    SHOP_INFO[id.to_string()].clone()
}

pub fn shop(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "shop_list": user["shop_list"].clone()
        }
    };
    global::send(resp, req)
}

pub fn buy(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let user_home = userdata::get_acc_home(&key);
    
    let item = get_item_info(body["master_shop_item_id"].as_i64().unwrap());
    
    items::remove_gems(&mut user, item["price"].as_i64().unwrap());
    items::give_shop(item["masterShopRewardId"].as_i64().unwrap(), item["price"].as_i64().unwrap(), &mut user);
    items::lp_modification(&mut user, item["price"].as_u64().unwrap() / 2, false);
    
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "gem": user["gem"].clone(),
            "shop_list": user["shop_list"].clone(),
            "gift_list": user_home["home"]["gift_list"].clone(),
            "updated_value_list": {
                "stamina": user["stamina"].clone()
            }
        }
    };
    global::send(resp, req)
}
