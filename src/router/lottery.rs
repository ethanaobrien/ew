use json::{array, object, JsonValue};
use actix_web::{HttpResponse, HttpRequest};
use rand::Rng;

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn tutorial(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let id = body["master_character_id"].to_string();
    let user = &id[id.len() - 2..].parse::<i32>().unwrap();
    let mut lotteryid = 9110000;
    if id.starts_with('2') {
        lotteryid += 9; //muse
    } else if id.starts_with('3') {
        lotteryid += 9 + 9; //aquors
    } else if id.starts_with('4') {
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
    global::send(resp, req)
}

fn get_card_master_id(lottery_id: String, lottery_number: String) -> Option<i64> {
    databases::CARDS[lottery_id][lottery_number]["value"].as_i64()
}
fn get_card(lottery_id: String, lottery_number: String) -> JsonValue {
    databases::CARDS[lottery_id][lottery_number].clone()
}

fn get_random_card(item: &JsonValue, rv: &mut JsonValue, rng: &mut rand::rngs::ThreadRng) {
    let lottery_id = item["masterLotteryItemId"].as_i64().unwrap();
    
    let mut random_id = 0;
    while random_id == 0 {
        let card = rng.gen_range(1..databases::POOL[lottery_id.to_string()][databases::POOL[lottery_id.to_string()].len() - 1].as_i64().unwrap() + 1);
        if get_card_master_id(lottery_id.to_string(), card.to_string()).is_some() {
            random_id = card;
            break;
        }
    }
    let to_push = object!{
        "id": get_card_master_id(lottery_id.to_string(), random_id.to_string()).unwrap(),
        "master_card_id": get_card_master_id(lottery_id.to_string(), random_id.to_string()).unwrap(),
        "master_lottery_item_id": lottery_id,
        "master_lottery_item_number": random_id
    };
    rv.push(to_push).unwrap();
}

fn get_random_cards(id: i64, mut count: usize) -> JsonValue {
    let total_ratio: i64 = databases::RARITY[id.to_string()].members().map(|item| if item["ensured"].as_i32().unwrap() == 1 { 0 } else { item["ratio"].as_i64().unwrap() }).sum();
    let mut rng = rand::thread_rng();
    let mut rv = array![];
    let mut promised = false;
    
    if count > 1 {
        for (_i, item) in databases::RARITY[id.to_string()].members().enumerate() {
            if item["ensured"].as_i32().unwrap() == 1 {
                get_random_card(item, &mut rv, &mut rng);
                promised = true;
                break;
            }
        }
    }
    if promised {
        count -= 1;
    }
    for _i in 0..count {
        let random_number: i64 = rng.gen_range(1..total_ratio + 1);
        let mut cumulative_ratio = 0;
        for (_i, item) in databases::RARITY[id.to_string()].members().enumerate() {
            cumulative_ratio += item["ratio"].as_i64().unwrap();
            if random_number <= cumulative_ratio {
                get_random_card(item, &mut rv, &mut rng);
                break;
            }
        }
    }
    rv
}

pub fn lottery(req: HttpRequest) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "lottery_list": []
        }
    };
    global::send(resp, req)
}

pub fn lottery_post(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    println!("lottery: {}", body);
    let mut user = userdata::get_acc(&key);
    let user2 = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut cleared_missions = array![];
    
    let lottery_id = body["master_lottery_id"].as_i64().unwrap();
    let price = databases::PRICE[lottery_id.to_string()][body["master_lottery_price_number"].to_string()].clone();
    
    if price["consumeType"].as_i32().unwrap() == 1 {
        items::remove_gems(&mut user, price["price"].as_i64().unwrap());
    } else if price["consumeType"].as_i32().unwrap() == 4 {
        items::use_item(price["masterItemId"].as_i64().unwrap(), price["price"].as_i64().unwrap(), &mut user);
    }
    
    let cardstogive = get_random_cards(lottery_id, price["count"].as_usize().unwrap());
    
    let lottery_type = databases::LOTTERY[lottery_id.to_string()]["category"].as_i32().unwrap();
    
    let mut new_cards = array![];
    let mut lottery_list = array![];
    
    if lottery_type == 1 {
        for (_i, data) in cardstogive.members().enumerate() {
            let mut is_new = true;
            if !items::give_character(data["master_card_id"].to_string(), &mut user, &mut missions, &mut cleared_missions) {
                is_new = false;
            }
            if is_new {
                let to_push = object!{
                    "id": data["master_card_id"].clone(),
                    "master_card_id": data["master_card_id"].clone(),
                    "exp": 0,
                    "skill_exp": 0,
                    "evolve": [],
                    "created_date_time": global::timestamp()
                };
                new_cards.push(to_push).unwrap();
            }
            let mut to_push = object!{
                "master_lottery_item_id": data["master_lottery_item_id"].clone(),
                "master_lottery_item_number": data["master_lottery_item_number"].clone(),
                "is_new": if is_new { 1 } else { 0 }
            };
            if !is_new {
                //given by global::give_character call
                to_push["exchange_item"] = object!{"master_item_id": 19100001, "amount": 50};
            }
            lottery_list.push(to_push).unwrap();
        }
        items::give_gift_basic(3, 15540034, 10, &mut user, &mut missions, &mut cleared_missions);
    } else if lottery_type == 2 {
        for (_i, data) in cardstogive.members().enumerate() {
            let info = get_card(data["master_lottery_item_id"].to_string(), data["master_lottery_item_number"].to_string());
            items::give_gift_basic(info["type"].as_i32().unwrap(), info["value"].as_i64().unwrap(), info["amount"].as_i64().unwrap(), &mut user, &mut missions, &mut cleared_missions);
            let to_push = object!{
                "master_lottery_item_id": data["master_lottery_item_id"].clone(),
                "master_lottery_item_number": data["master_lottery_item_number"].clone(),
                "is_new": 0
            };
            lottery_list.push(to_push).unwrap();
        }
    }
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, missions);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "lottery_item_list": lottery_list,
            "updated_value_list": {
                "card_list": new_cards,
                "item_list": user["item_list"].clone()
            },
            "gift_list": user2["home"]["gift_list"].clone(),
            "clear_mission_ids": cleared_missions,
            "draw_count_list": []
        }
    };
    global::send(resp, req)
}
