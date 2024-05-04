use json::{JsonValue, object};
use actix_web::{HttpResponse, HttpRequest};
use lazy_static::lazy_static;

use crate::router::{global, userdata, items};
use crate::encryption;

lazy_static! {
    static ref EXCHANGE_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/exchange_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    static ref EXCHANGE_REWARD: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/exchange_item_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
}

pub fn exchange(req: HttpRequest) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {"exchange_list":[]}
    };
    global::send(resp, req)
}

pub fn exchange_post(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    let item = &EXCHANGE_LIST[body["master_exchange_item_id"].to_string()];
    
    if item["consumeType"].as_i32().unwrap() == 4 {
        items::use_item(item["value"].as_i64().unwrap(), item["amount"].as_i64().unwrap() * body["count"].as_i64().unwrap(), &mut user);
    } else {
        println!("Unknown consume type {}", item["consumeType"]);
    }
    
    let mut gift = EXCHANGE_REWARD[item["masterExchangeItemRewardId"].to_string()].clone();
    gift["reward_type"] = gift["type"].clone();
    gift["amount"] = (gift["amount"].as_i64().unwrap() * body["count"].as_i64().unwrap()).into();
    items::give_gift(&gift, &mut user);
    
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "exchange": body,
            "updated_value_list": {
                "card_list": user["card_list"].clone(),
                "item_list": user["item_list"].clone(),
                "point_list": user["point_list"].clone()
            },
            "clear_mission_ids": []
        }
    };
    global::send(resp, req)
}
