use json::{array, object, JsonValue};
use actix_web::{HttpResponse, HttpRequest};
use lazy_static::lazy_static;
use rand::Rng;

use crate::router::{global, userdata, items};
use crate::encryption;

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
    static ref POOL: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(include_str!("lottery_item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
            }
            cardz[data["id"].to_string()].push(data["number"].clone()).unwrap();
        }
        cardz
    };
    static ref RARITY: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(include_str!("lottery_rarity.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            if cardz[data["id"].to_string()].is_null() {
                cardz[data["id"].to_string()] = array![];
            }
            cardz[data["id"].to_string()].push(data.clone()).unwrap();
        }
        cardz
    };
    static ref LOTTERY: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(include_str!("lottery.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            cardz[data["id"].to_string()] = data.clone();
        }
        cardz
    };
    static ref PRICE: JsonValue = {
        let mut cardz = object!{};
        let items = json::parse(include_str!("lottery_price.json")).unwrap();
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

fn get_card_master_id(lottery_id: String, lottery_number: String) -> Option<i64> {
    CARDS[lottery_id][lottery_number]["value"].as_i64()
}
fn get_card(lottery_id: String, lottery_number: String) -> JsonValue {
    CARDS[lottery_id][lottery_number].clone()
}

fn get_random_cards(id: i64, count: usize) -> JsonValue {
    let total_ratio: i64 = RARITY[id.to_string()].members().into_iter().map(|item| item["ratio"].as_i64().unwrap()).sum();
    let mut rng = rand::thread_rng();
    let mut rv = array![];
    for _i in 0..count {
        let random_number: i64 = rng.gen_range(1..total_ratio + 1);
        let mut cumulative_ratio = 0;
        for (_i, item) in RARITY[id.to_string()].members().enumerate() {
            cumulative_ratio += item["ratio"].as_i64().unwrap();
            if random_number <= cumulative_ratio {
                let lottery_id = item["masterLotteryItemId"].as_i64().unwrap();
                
                let mut random_id = 0;
                while random_id == 0 {
                    let card = rng.gen_range(1..POOL[lottery_id.to_string()][POOL[lottery_id.to_string()].len() - 1].as_i64().unwrap() + 1);
                    if !get_card_master_id(lottery_id.to_string(), card.to_string()).is_none() {
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
                break;
            }
        }
    }
    rv
}

pub fn lottery(_req: HttpRequest) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "lottery_list": []
        }
    };
    global::send(resp)
}

pub fn lottery_post(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    println!("lottery: {}", body);
    let mut user = userdata::get_acc(&key);
    let user2 = userdata::get_acc(&key);
    
    let mut cardstogive;
    
    let lottery_id = body["master_lottery_id"].as_i64().unwrap();
    if user["tutorial_step"].as_i32().unwrap() != 130 {
        cardstogive = get_random_cards(body["master_lottery_id"].as_i64().unwrap(), 9);
        let item_id = (body["master_lottery_id"].to_string().parse::<i32>().unwrap() * 100) + 1;
        //tutorial
        let new_card = object!{
            "master_card_id": get_card_master_id(item_id.to_string(), String::from("1")).unwrap(),
            "master_lottery_item_id": item_id,
            "master_lottery_item_number": 1
        };
        cardstogive.push(new_card).unwrap();
    } else {
        let price = PRICE[lottery_id.to_string()][body["master_lottery_price_number"].to_string()].clone();
        
        if price["consumeType"].as_i32().unwrap() == 1 {
            items::remove_gems(&mut user, price["price"].as_i64().unwrap());
        } else if price["consumeType"].as_i32().unwrap() == 4 {
            items::use_item(price["masterItemId"].as_i64().unwrap(), price["price"].as_i64().unwrap(), &mut user);
        }
        
        cardstogive = get_random_cards(lottery_id, price["count"].as_usize().unwrap());
    }
    
    let lottery_type = LOTTERY[lottery_id.to_string()]["category"].as_i32().unwrap();
    
    let mut new_cards = array![];
    let mut lottery_list = array![];
    
    if lottery_type == 1 {
        for (_i, data) in cardstogive.members().enumerate() {
            let mut is_new = true;
            if !items::give_character(data["master_card_id"].to_string(), &mut user) {
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
        items::give_gift_basic(3, 15540034, 10, &mut user);
    } else if lottery_type == 2 {
        for (_i, data) in cardstogive.members().enumerate() {
            let info = get_card(data["master_lottery_item_id"].to_string(), data["master_lottery_item_number"].to_string());
            items::give_gift_basic(info["type"].as_i32().unwrap(), info["value"].as_i64().unwrap(), info["amount"].as_i64().unwrap(), &mut user);
            let to_push = object!{
                "master_lottery_item_id": data["master_lottery_item_id"].clone(),
                "master_lottery_item_number": data["master_lottery_item_number"].clone(),
                "is_new": 0
            };
            lottery_list.push(to_push).unwrap();
        }
    }
    
    userdata::save_acc(&key, user.clone());
    
    //todo
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
            "clear_mission_ids": user2["clear_mission_ids"].clone(),
            "draw_count_list": []
        }
    };
    global::send(resp)
}
