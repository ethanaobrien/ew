use json::object;
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{userdata, global, items, databases};
use crate::encryption;

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
    
    let item = &databases::SHOP_INFO[body["master_shop_item_id"].to_string()];
    
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
