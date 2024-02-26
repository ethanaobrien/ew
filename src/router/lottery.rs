use json;
use json::{array, object};
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

pub fn tutorial(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut lotteryid = 0; //todo, what should this be?
    if body["master_character_id"].to_string().starts_with("3") {
        lotteryid = 9110027;
    } else {
        println!("Unknown lottery id for character {}", body["master_character_id"]);
    }
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "lottery_list": [
                {
                    "master_lottery_id": lotteryid,
                    "master_lottery_price_number": 1,
                    "count": 0,
                    "daily_count": 0,
                    "last_count_date": ""
                }
            ],
            "item_list": []
        }
    };
    global::send(resp)
}

pub fn lottery(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    println!("lottery: {}", body);
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let mut user = userdata::get_acc(key, uid);
    let user2 = userdata::get_acc(key, uid);
    
    //todo - how to randomize?
    let cardstogive = array![
        {"id": 13930, "master_card_id": 10010011, "master_lottery_item_id":100001, "master_lottery_item_number":138},
        {"id": 13928, "master_card_id": 10030008, "master_lottery_item_id":200001,"master_lottery_item_number":30},
        {"id": 13926, "master_card_id": 20010010, "master_lottery_item_id":100001,"master_lottery_item_number":178},
        {"id": 13929, "master_card_id": 20050004, "master_lottery_item_id":100001,"master_lottery_item_number":26},
        {"id": 13925, "master_card_id": 20090001, "master_lottery_item_id":100001,"master_lottery_item_number":113},
        {"id": 13933, "master_card_id": 30040001, "master_lottery_item_id":200001,"master_lottery_item_number":2},
        {"id": 13934, "master_card_id": 30090007, "master_lottery_item_id":200001,"master_lottery_item_number":83},
        {"id": 13931, "master_card_id": 30100005, "master_lottery_item_id":100001,"master_lottery_item_number":188},
        {"id": 13927, "master_card_id": 30120001, "master_lottery_item_id":100001,"master_lottery_item_number":154},
        {"id": 13932, "master_card_id": 40030002, "master_lottery_item_id":911002701,"master_lottery_item_number":1}
    ];
    
    let mut new_cards = array![];
    for (i, data) in cardstogive.members().enumerate() {
        let to_push = object!{
            "id": data["id"].clone(),
            "master_card_id": data["master_card_id"].clone(),
            "exp":0,
            "skill_exp":0,
            "evolve":[],
            "created_date_time": global::timestamp()
        };
        user["card_list"].push(to_push.clone()).unwrap();
        new_cards.push(to_push).unwrap();
        user["deck_list"][0]["main_card_ids"][i] = data["id"].clone();
    }
    
    userdata::save_acc(key, uid, user);
    
    let mut lottery_list = array![];
    for (_i, data) in cardstogive.members().enumerate() {
        let to_push = object!{
            "master_lottery_item_id": data["master_lottery_item_id"].clone(),
            "master_lottery_item_number": data["master_lottery_item_number"].clone(),
            "is_new": 1
        };
        lottery_list.push(to_push).unwrap();
    }
    
    //todo
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "lottery_item_list": lottery_list,
            "updated_value_list": {
                "card_list": new_cards
            },
            "gift_list": user2["home"]["gift_list"].clone(),
            "clear_mission_ids": user2["clear_mission_ids"].clone(),
            "draw_count_list": []
        }
    };
    global::send(resp)
}
