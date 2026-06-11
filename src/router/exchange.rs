use jzon::{object, array};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/exchange").route(web::get().to(exchange)).route(web::post().to(exchange_post)));
}

async fn exchange(req: HttpRequest) -> impl Responder {
    global::api(&req, Some(object!{"exchange_list":[]}))
}

async fn exchange_post(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut cleared_missions = array![];
    
    let item = &databases::EXCHANGE_LIST[body["master_exchange_item_id"].to_string()];
    
    items::use_item(item, body["count"].as_i64().unwrap(), &mut user);
    
    let mut gift = databases::EXCHANGE_REWARD[item["masterExchangeItemRewardId"].to_string()].clone();
    gift["reward_type"] = gift["type"].clone();
    gift["amount"] = (gift["amount"].as_i64().unwrap() * body["count"].as_i64().unwrap()).into();
    items::give_gift(&gift, &mut user, &mut missions, &mut cleared_missions, &mut chats);
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, missions);
    userdata::save_acc_chats(&key, chats);

    global::api(&req, Some(object!{
        "exchange": body,
        "updated_value_list": {
            "card_list": user["card_list"].clone(),
            "item_list": user["item_list"].clone(),
            "point_list": user["point_list"].clone()
        },
        "clear_mission_ids": cleared_missions
    }))
}
