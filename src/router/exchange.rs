use json::{object, array, JsonValue};
use actix_web::{HttpRequest};

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn exchange(_req: HttpRequest) -> Option<JsonValue> {
    Some(object!{"exchange_list":[]})
}

pub fn exchange_post(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut cleared_missions = array![];
    
    let item = &databases::EXCHANGE_LIST[body["master_exchange_item_id"].to_string()];
    
    if item["consumeType"].as_i32().unwrap() == 4 {
        items::use_item(item["value"].as_i64().unwrap(), item["amount"].as_i64().unwrap() * body["count"].as_i64().unwrap(), &mut user);
    } else {
        println!("Unknown consume type {}", item["consumeType"]);
    }
    
    let mut gift = databases::EXCHANGE_REWARD[item["masterExchangeItemRewardId"].to_string()].clone();
    gift["reward_type"] = gift["type"].clone();
    gift["amount"] = (gift["amount"].as_i64().unwrap() * body["count"].as_i64().unwrap()).into();
    items::give_gift(&gift, &mut user, &mut missions, &mut cleared_missions);
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, missions);
    
    Some(object!{
        "exchange": body,
        "updated_value_list": {
            "card_list": user["card_list"].clone(),
            "item_list": user["item_list"].clone(),
            "point_list": user["point_list"].clone()
        },
        "clear_mission_ids": cleared_missions
    })
}
