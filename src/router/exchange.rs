use jzon::{object, array, JsonValue};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/exchange").route(web::get().to(exchange)).route(web::post().to(exchange_post)));
}

async fn exchange(req: HttpRequest) -> impl Responder {
    let key = global::get_login(req.headers(), "");
    let exchange = userdata::get_acc_exchange(&key);
    global::api(&req, Some(object!{
        "exchange_list": exchange
    }))
}

fn exchange_updated_value_list(is_card: bool, granted_id: i64, consumed_id: i64, user: &mut JsonValue) -> JsonValue {
    if consumed_id != 0 && !user["item_list"].members().any(|it| it["master_item_id"].as_i64() == Some(consumed_id)) {
        items::give_item(consumed_id, 0, user);
    }
    let mut item_list = array![];
    for it in user["item_list"].members() {
        let mid = it["master_item_id"].as_i64();
        if mid == Some(granted_id) || mid == Some(consumed_id) {
            item_list.push(it.clone()).unwrap();
        }
    }
    let mut rv = object!{ item_list: item_list };
    if is_card {
        rv["card_list"] = user["card_list"].clone();
    }
    rv
}

async fn exchange_post(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut exchanges = userdata::get_acc_exchange(&key);
    let mut cleared_missions = array![];

    let exchange_id = body["master_exchange_item_id"].as_i64().unwrap();
    let count = body["count"].as_i64().unwrap();
    let item = databases::EXCHANGE_LIST[exchange_id.to_string()].clone();
    let consumed_id = item["value"].as_i64().unwrap_or(0);

    items::use_item(&item, count, &mut user);

    let mut gift = databases::EXCHANGE_REWARD[item["masterExchangeItemRewardId"].to_string()].clone();
    gift["reward_type"] = gift["type"].clone();
    gift["amount"] = (gift["amount"].as_i64().unwrap() * count).into();
    let granted_id = gift["value"].as_i64().unwrap_or(0);
    let is_card = gift["type"] == 2;
    items::give_gift(&gift, &mut user, &mut missions, &mut cleared_missions, &mut chats);

    let mut total = count;
    let mut found = false;
    for entry in exchanges.members_mut() {
        if entry["master_exchange_item_id"].as_i64() == Some(exchange_id) {
            total = entry["count"].as_i64().unwrap() + count;
            entry["count"] = total.into();
            found = true;
            break;
        }
    }
    if !found {
        exchanges.push(object!{
            master_exchange_item_id: exchange_id,
            count: total
        }).unwrap();
    }

    let updated_value_list = exchange_updated_value_list(is_card, granted_id, consumed_id, &mut user);

    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, missions);
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_exchange(&key, exchanges);

    global::api(&req, Some(object!{
        "clear_mission_ids": cleared_missions,
        "exchange": {
            master_exchange_item_id: exchange_id,
            count: total
        },
        "updated_value_list": updated_value_list
    }))
}
