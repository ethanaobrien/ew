use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, Responder};
use rand::RngExt;

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/lottery")
            .service(web::resource("").route(web::get().to(lottery)).route(web::post().to(lottery_post)))
            .route("/get_tutorial", web::post().to(tutorial))
    );
}

async fn tutorial(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
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
    
    global::api(&req, Some(object!{
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
    }))
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
        let card = rng.random_range(1..databases::POOL[lottery_id.to_string()][databases::POOL[lottery_id.to_string()].len() - 1].as_i64().unwrap() + 1);
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
    let rarity = &databases::RARITY[id.to_string()];
    let total_ratio: i64 = rarity.members().map(|item| if item["ensured"].as_i32().unwrap() == 1 { 0 } else { item["ratio"].as_i64().unwrap() }).sum();
    let ensured_ratio: i64 = rarity.members().map(|item| if item["ensured"].as_i32().unwrap() == 1 { item["ratio"].as_i64().unwrap() } else { 0 }).sum();
    let mut rng = rand::rng();
    let mut rv = array![];

    if count > 1 && ensured_ratio > 0 {
        let random_number: i64 = rng.random_range(1..ensured_ratio + 1);
        let mut cumulative_ratio = 0;
        for item in rarity.members() {
            if item["ensured"].as_i32().unwrap() != 1 {
                continue;
            }
            cumulative_ratio += item["ratio"].as_i64().unwrap();
            if random_number <= cumulative_ratio {
                get_random_card(item, &mut rv, &mut rng);
                count -= 1;
                break;
            }
        }
    }
    for _i in 0..count {
        let random_number: i64 = rng.random_range(1..total_ratio + 1);
        let mut cumulative_ratio = 0;
        for item in rarity.members() {
            if item["ensured"].as_i32().unwrap() == 1 {
                continue;
            }
            cumulative_ratio += item["ratio"].as_i64().unwrap();
            if random_number <= cumulative_ratio {
                get_random_card(item, &mut rv, &mut rng);
                break;
            }
        }
    }
    rv
}

fn lottery_day() -> i64 {
    (global::timestamp() as i64 + 32400) / 86400
}

fn get_draw_count(user: &JsonValue, lottery_id: i64, price_number: i64) -> i64 {
    for data in user["lottery_list"].members() {
        if data["master_lottery_id"].as_i64() == Some(lottery_id) && data["master_lottery_price_number"].as_i64() == Some(price_number) {
            return data["count"].as_i64().unwrap_or(0);
        }
    }
    0
}

fn add_draw_count(user: &mut JsonValue, lottery_id: i64, price_number: i64) {
    let today = lottery_day();
    if !user["lottery_list"].is_array() {
        user["lottery_list"] = array![];
    }
    for data in user["lottery_list"].members_mut() {
        if data["master_lottery_id"].as_i64() == Some(lottery_id) && data["master_lottery_price_number"].as_i64() == Some(price_number) {
            let daily = if data["last_count_date"].as_i64() == Some(today) { data["daily_count"].as_i64().unwrap_or(0) } else { 0 };
            data["count"] = (data["count"].as_i64().unwrap_or(0) + 1).into();
            data["daily_count"] = (daily + 1).into();
            data["last_count_date"] = today.into();
            return;
        }
    }
    user["lottery_list"].push(object!{
        "master_lottery_id": lottery_id,
        "master_lottery_price_number": price_number,
        "count": 1,
        "daily_count": 1,
        "last_count_date": today
    }).unwrap();
}

fn is_stepup(lottery_id: i64) -> bool {
    databases::LOTTERY[lottery_id.to_string()]["type"].as_i64() == Some(2)
}

fn stepup_step(lottery_id: i64, draws: i64) -> JsonValue {
    let steps = &databases::STEPUP[lottery_id.to_string()];
    let step = draws % steps.len() as i64 + 1;
    steps.members().find(|n| n["count"].as_i64() == Some(step)).unwrap().clone()
}

fn get_lottery_list(user: &JsonValue) -> JsonValue {
    let today = lottery_day();
    let mut rv = array![];
    for data in user["lottery_list"].members() {
        let lottery_id = data["master_lottery_id"].as_i64().unwrap_or(0);
        let price_number = data["master_lottery_price_number"].as_i64().unwrap_or(0);
        let mut count = data["count"].as_i64().unwrap_or(0);
        if price_number == 1 && is_stepup(lottery_id) {
            count += 1;
        }
        let daily = if data["last_count_date"].as_i64() == Some(today) { data["daily_count"].as_i64().unwrap_or(0) } else { 0 };
        rv.push(object!{
            "master_lottery_id": lottery_id,
            "master_lottery_price_number": price_number,
            "count": count,
            "daily_count": daily,
            "last_count_date": ""
        }).unwrap();
    }
    for entry in databases::STEPUP.entries() {
        let lottery_id = entry.0.parse::<i64>().unwrap();
        if rv.members().any(|data| data["master_lottery_id"].as_i64() == Some(lottery_id) && data["master_lottery_price_number"].as_i64() == Some(1)) {
            continue;
        }
        rv.push(object!{
            "master_lottery_id": lottery_id,
            "master_lottery_price_number": 1,
            "count": 1,
            "daily_count": 0,
            "last_count_date": ""
        }).unwrap();
    }
    rv
}

async fn lottery(req: HttpRequest) -> impl Responder {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    global::api(&req, Some(object!{
        "lottery_list": get_lottery_list(&user)
    }))
}

async fn lottery_post(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    //println!("lottery: {}", body);
    let mut user = userdata::get_acc(&key);
    let user2 = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut cleared_missions = array![];
    
    let lottery_id = body["master_lottery_id"].as_i64().unwrap();
    let price_number = body["master_lottery_price_number"].as_i64().unwrap();

    let lottery = &databases::LOTTERY[lottery_id.to_string()];
    let lottery_type = lottery["category"].as_i32().unwrap();
    let exchange_id = lottery["exchangeMasterItemId"].as_i64().unwrap_or(0);

    let (price_id, rarity_id) = if is_stepup(lottery_id) && price_number == 1 {
        let step = stepup_step(lottery_id, get_draw_count(&user, lottery_id, 1));
        (step["masterLotteryPriceId"].as_i64().unwrap(), step["masterLotteryRarityId"].as_i64().unwrap())
    } else {
        (lottery["masterLotteryPriceId"].as_i64().unwrap_or(lottery_id), lottery["masterLotteryRarityId"].as_i64().unwrap_or(lottery_id))
    };
    let price = databases::PRICE[price_id.to_string()][price_number.to_string()].clone();

    items::use_item(&object!{
        value: price["masterItemId"].clone(),
        amount: price["price"].clone(),
        consumeType: price["consumeType"].clone()
    }, 1, &mut user);

    let count = price["count"].as_usize().unwrap();

    let cardstogive = get_random_cards(rarity_id, count);

    let mut new_cards = array![];
    let mut lottery_list = array![];
    
    if lottery_type == 1 {
        for data in cardstogive.members() {
            let mut is_new = true;
            if !items::give_character(data["master_card_id"].as_i64().unwrap(), &mut user, &mut missions, &mut cleared_missions, &mut chats) {
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
                let character_rarity = items::get_rarity(data["master_card_id"].as_i64().unwrap());
                let amount = if character_rarity == 1 { 20 } else if character_rarity == 2 { 50 } else if character_rarity == 3 { 500 } else { 0 };
                to_push["exchange_item"] = object!{
                    master_item_id: 19100001,
                    amount: amount
                };
            }
            lottery_list.push(to_push).unwrap();
        }
    } else if lottery_type == 2 {
        for data in cardstogive.members() {
            let info = get_card(data["master_lottery_item_id"].to_string(), data["master_lottery_item_number"].to_string());
            items::give_gift_basic(info["type"].as_i32().unwrap(), info["value"].as_i64().unwrap(), info["amount"].as_i64().unwrap(), &mut user, &mut missions, &mut cleared_missions, &mut chats);
            let to_push = object!{
                "master_lottery_item_id": data["master_lottery_item_id"].clone(),
                "master_lottery_item_number": data["master_lottery_item_number"].clone(),
                "is_new": 0
            };
            lottery_list.push(to_push).unwrap();
        }
    }

    if exchange_id != 0 {
        items::give_gift_basic(3, exchange_id, 10, &mut user, &mut missions, &mut cleared_missions, &mut chats);
    }

    add_draw_count(&mut user, lottery_id, price_number);
    let mut new_count = get_draw_count(&user, lottery_id, price_number);
    if is_stepup(lottery_id) && price_number == 1 {
        new_count += 1;
    }

    userdata::save_acc(&key, user.clone());
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_missions(&key, missions);

    global::api(&req, Some(object!{
        "lottery_item_list": lottery_list,
        "updated_value_list": {
            "card_list": new_cards,
            "item_list": user["item_list"].clone()
        },
        "gift_list": user2["home"]["gift_list"].clone(),
        "clear_mission_ids": cleared_missions,
        "draw_count_list": [
            {
                "number": price_number,
                "count": new_count
            }
        ]
    }))
}



