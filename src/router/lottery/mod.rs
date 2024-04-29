use json;
use json::{array, object, JsonValue};
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;
use lazy_static::lazy_static;
use rand::Rng;

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

fn get_card_master_id(lottery_id: String, lottery_number: String) -> Option<i64> {
    CARDS[lottery_id][lottery_number]["value"].as_i64()
}
fn random_number(lowest: usize, highest: usize) -> usize {
    if lowest == highest {
        return lowest;
    }
    assert!(lowest < highest);
    
    rand::thread_rng().gen_range(lowest..highest + 1)
}

//todo - how to randomize?
fn get_random_cards(count: usize) -> JsonValue {
    let pools = array![[100001, 207], [200001, 117], [300001, 39]];
    let mut random_master_ids = array![
        // [master_lottery_item_id, master_lottery_item_number]
    ];
    let mut i=0;
    while i < count {
        let pool = pools[random_number(0, pools.len()-1)].clone();
        let card = random_number(0, pool[1].as_usize().unwrap());
        if !get_card_master_id(pool[0].to_string(), card.to_string()).is_none() {
            random_master_ids.push(array![pool[0].clone(), card]).unwrap();
            i += 1;
        }
    }
    let mut rv = array![];
    for (_i, data) in random_master_ids.members().enumerate() {
        let to_push = object!{
            "id": get_card_master_id(data[0].to_string(), data[1].to_string()).unwrap(),
            "master_card_id": get_card_master_id(data[0].to_string(), data[1].to_string()).unwrap(),
            "master_lottery_item_id": data[0].clone(),
            "master_lottery_item_number": data[1].clone()
        };
        rv.push(to_push).unwrap();
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
    
    if user["tutorial_step"].to_string() != "130" && body["master_lottery_id"].to_string().starts_with("9") {
        cardstogive = get_random_cards(9);
        let item_id = (body["master_lottery_id"].to_string().parse::<i32>().unwrap() * 100) + 1;
        //tutorial
        let new_card = object!{
            "master_card_id": get_card_master_id(item_id.to_string(), String::from("1")).unwrap(),
            "master_lottery_item_id": item_id,
            "master_lottery_item_number": 1
        };
        cardstogive.push(new_card).unwrap();
    } else {
        cardstogive = get_random_cards(10);
    }
    
    let mut new_cards = array![];
    let mut new_ids = array![];
    for (_i, data) in cardstogive.members().enumerate() {
        if !global::give_character(data["master_card_id"].to_string(), &mut user) {
            global::give_item(19100001, 20, &mut user);
            continue;
        }
        new_ids.push(data["master_lottery_item_id"].to_string()).unwrap();
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
    
    userdata::save_acc(&key, user.clone());
    
    let mut lottery_list = array![];
    for (_i, data) in cardstogive.members().enumerate() {
        let new = if new_ids.contains(data["master_lottery_item_id"].to_string()) { 1 } else { 0 };
        let to_push = object!{
            "master_lottery_item_id": data["master_lottery_item_id"].clone(),
            "master_lottery_item_number": data["master_lottery_item_number"].clone(),
            "is_new": new
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
