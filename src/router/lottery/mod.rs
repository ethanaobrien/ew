use json;
use json::{array, object, JsonValue};
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;
use lazy_static::lazy_static;

lazy_static! {
    static ref CARDS: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(include_str!("lottery_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = object!{};
            }
            cardz[data["id"].to_string()][data["number"].to_string()] = data.clone();
        }
        cardz
    };
}

pub fn tutorial(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let id = body["master_character_id"].to_string();
    let user = &id[id.len() - 2..].parse::<i32>().unwrap();
    let mut lotteryid = 9110000;
    if id.starts_with("2") {
        lotteryid += 9; //muse
    } else if id.starts_with("3") {
        lotteryid += 9 + 9; //aquors
    } else if id.starts_with("4") {
        lotteryid += 9 + 9 + 12; //nijigasaki
    }
    lotteryid += user;
    
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

fn get_card_master_id(lottery_id: String, lottery_number: String) -> i32 {
    return CARDS[lottery_id][lottery_number]["value"].as_i32().unwrap();
}

//todo - how to randomize?
fn get_random_cards(_count: i32) -> JsonValue {
    let random_master_ids = array![
        // [master_lottery_item_id, master_lottery_item_number]
        [100001, 138],
        [200001, 30],
        [100001, 178],
        [100001, 26],
        [100001, 113],
        [200001, 2],
        [200001, 83],
        [100001, 188],
        [100001, 154]
    ];
    let mut rv = array![];
    for (_i, data) in random_master_ids.members().enumerate() {
        let to_push = object!{
            "id": format!("{}{}", data[0].to_string(), data[1].to_string()),
            "master_card_id": get_card_master_id(data[0].to_string(), data[1].to_string()),
            "master_lottery_item_id": data[0].clone(),
            "master_lottery_item_number": data[1].clone()
        };
        rv.push(to_push).unwrap();
    }
    rv
}

pub fn lottery(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    println!("lottery: {}", body);
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let mut user = userdata::get_acc(key);
    let user2 = userdata::get_acc(key);
    
    let mut cardstogive = get_random_cards(9);
    /*let cardstogive = array![
    //30110007
        {"id": 10, "master_card_id": 40030002, "master_lottery_item_id":911002701,"master_lottery_item_number":1}
    ];*/
    if body["master_lottery_id"].to_string().starts_with("9") {
        let item_id = (body["master_lottery_id"].to_string().parse::<i32>().unwrap() * 100) + 1;
        //tutorial
        let new_card = object!{
            "id": 15500,
            "master_card_id": get_card_master_id(item_id.to_string(), String::from("1")),
            "master_lottery_item_id": item_id,
            "master_lottery_item_number": 1
        };
        cardstogive.push(new_card).unwrap();
    }
    
    let mut new_cards = array![];
    for (i, data) in cardstogive.members().enumerate() {
        let to_push = object!{
            "id": data["id"].clone(),
            "master_card_id": data["master_card_id"].clone(),
            "exp": 0,
            "skill_exp":0,
            "evolve":[],
            "created_date_time": global::timestamp()
        };
        user["card_list"].push(to_push.clone()).unwrap();
        new_cards.push(to_push).unwrap();
        if i < 9 {
            user["deck_list"][0]["main_card_ids"][i] = data["id"].to_string().parse::<i32>().unwrap().into();
        }
    }
    user["deck_list"][0]["main_card_ids"][8] = cardstogive[9]["id"].to_string().parse::<i32>().unwrap().into();
    
    userdata::save_acc(key, user);
    
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
