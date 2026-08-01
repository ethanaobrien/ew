use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, Responder};
use rand::RngExt;

use crate::router::{global, userdata, items, databases, custom_card, Body, Login, Session, Api};
use crate::database::custom_card as custom_card_db;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/lottery")
            .service(web::resource("").route(web::get().to(lottery)).route(web::post().to(lottery_post)))
            .route("/get_tutorial", web::post().to(tutorial))
    );
}

async fn tutorial(Body(body): Body) -> impl Responder {
    
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
    
    Api(Some(object!{
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

// The runtime custom-card banner (lottery id 6900001). The CLIENT synthesizes
// its lottery/price/rarity/item masterdata from the catalog; the server only
// handles the draw. Cost and rarity structure mirror the baked SIF1-import
// banners 6110001-6110004 (lottery_price.csv / lottery_rarity.csv):
//   price 1 = 11 draws / 3000 free gems, price 2 = 1 draw / 300 free gems
//   normal roll r1 6800 / r2 2600 / r3 600; multi draws replace one roll with
//   an ensured r2-or-better at 8125 / 1875
// Wire contract for the drawn items (the client synthesizes matching
// LotteryItemMst rows): master_lottery_item_id = 690000100 + rarity,
// master_lottery_item_number = master_card_id - 150000000
const CUSTOM_BANNER_RATIO: &[(i64, i64)] = &[(1, 6800), (2, 2600), (3, 600)];
const CUSTOM_BANNER_ENSURED: &[(i64, i64)] = &[(2, 8125), (3, 1875)];

fn custom_banner_price(price_number: i64) -> JsonValue {
    match price_number {
        1 => object!{"masterItemId": 0, "consumeType": 1, "count": 11, "price": 3000},
        2 => object!{"masterItemId": 0, "consumeType": 1, "count": 1, "price": 300},
        _ => JsonValue::Null
    }
}

// One roll: weighted rarity over the rarities that actually have published +
// obtainable cards, then a uniform card within the rarity. None when every
// pool is empty
fn custom_banner_roll(table: &[(i64, i64)], pools: &[Vec<i64>; 3], rng: &mut rand::rngs::ThreadRng) -> Option<(i64, i64)> {
    let available: Vec<&(i64, i64)> = table.iter().filter(|(rarity, _)| !pools[(*rarity - 1) as usize].is_empty()).collect();
    let total: i64 = available.iter().map(|(_, ratio)| ratio).sum();
    if total <= 0 {
        return None;
    }
    let roll = rng.random_range(1..=total);
    let mut cumulative = 0;
    for (rarity, ratio) in available {
        cumulative += ratio;
        if roll <= cumulative {
            let pool = &pools[(*rarity - 1) as usize];
            return Some((*rarity, pool[rng.random_range(0..pool.len())]));
        }
    }
    None
}

// The draw, in the same result shape get_random_cards produces so the stock
// grant loop consumes it unchanged. Empty when there is nothing obtainable -
// the caller bails before charging
fn custom_banner_cards(count: usize) -> JsonValue {
    let pools: [Vec<i64>; 3] = [
        custom_card_db::obtainable_card_ids(1),
        custom_card_db::obtainable_card_ids(2),
        custom_card_db::obtainable_card_ids(3)
    ];
    let mut rng = rand::rng();
    let mut rv = array![];
    let mut remaining = count;
    if count > 1 {
        // The ensured slot falls back to a normal roll when no r2/r3 exists
        if let Some((rarity, card)) = custom_banner_roll(CUSTOM_BANNER_ENSURED, &pools, &mut rng)
            .or_else(|| custom_banner_roll(CUSTOM_BANNER_RATIO, &pools, &mut rng)) {
            rv.push(object!{
                "id": card,
                "master_card_id": card,
                "master_lottery_item_id": 690_000_100 + rarity,
                "master_lottery_item_number": card - 150_000_000
            }).unwrap();
            remaining -= 1;
        }
    }
    for _ in 0..remaining {
        let Some((rarity, card)) = custom_banner_roll(CUSTOM_BANNER_RATIO, &pools, &mut rng) else { break; };
        rv.push(object!{
            "id": card,
            "master_card_id": card,
            "master_lottery_item_id": 690_000_100 + rarity,
            "master_lottery_item_number": card - 150_000_000
        }).unwrap();
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

async fn lottery(Login(key): Login) -> impl Responder {
    let user = userdata::get_acc(&key);
    Api(Some(object!{
        "lottery_list": get_lottery_list(&user)
    }))
}

async fn lottery_post(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    //println!("lottery: {}", body);
    let mut user = userdata::get_acc(&key);
    let user2 = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut cleared_missions = array![];
    
    let lottery_id = body["master_lottery_id"].as_i64().unwrap();
    let price_number = body["master_lottery_price_number"].as_i64().unwrap();

    // Custom lotteries live in the 6M band (below TUTORIAL_MIN_LOTTERY_ID; 1-5M/7M/8M official)
    if (6_000_000..7_000_000).contains(&lottery_id) && !crate::router::card::client_supports_custom_cards(&req) {
        return global::api_error(&req, global::RESULT_GAME_VERSION_UPDATED);
    }
    // The runtime custom-card banner additionally needs the catalog protocol
    let is_custom_banner = lottery_id == custom_card::CUSTOM_LOTTERY_ID;
    if is_custom_banner && (custom_card::disabled() || !custom_card::client_supports(&req)) {
        return global::api_error(&req, global::RESULT_GAME_VERSION_UPDATED);
    }

    let (price, cardstogive, lottery_type, exchange_id) = if is_custom_banner {
        let price = custom_banner_price(price_number);
        if price.is_null() {
            return global::api_error(&req, global::RESULT_GAME_VERSION_UPDATED);
        }
        let drawn = custom_banner_cards(price["count"].as_usize().unwrap());
        if drawn.is_empty() {
            // Nothing published + obtainable: nothing charged, nothing drawn.
            // The client only synthesizes the banner when the pool is
            // non-empty, so this is a stale-catalog race, not a normal path
            return global::api(&req, Some(object!{
                "lottery_item_list": [],
                "updated_value_list": {},
                "gift_list": user2["home"]["gift_list"].clone(),
                "clear_mission_ids": [],
                "draw_count_list": []
            }));
        }
        (price, drawn, 1, 0)
    } else {
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
        let count = price["count"].as_usize().unwrap();
        (price, get_random_cards(rarity_id, count), lottery_type, exchange_id)
    };

    items::use_item(&object!{
        value: price["masterItemId"].clone(),
        amount: price["price"].clone(),
        consumeType: price["consumeType"].clone()
    }, 1, &mut user);

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

    // An account holding a runtime custom card needs a catalog-fetching
    // client from now on; start.rs enforces it (monotonic, like the level-2
    // flag for the baked band)
    if cardstogive.members().any(|card| custom_card::is_custom_runtime(card["master_card_id"].as_i64().unwrap_or(0))) {
        userdata::save_protocol_version(&key, custom_card::PROTOCOL_VERSION);
    }

    global::api(&req, Some(object!{
        "lottery_item_list": lottery_list,
        "updated_value_list": {
            // The draw can charge gems (the custom banners do); without this
            // the client's balance drifts until the next /api/user pull
            "gem": user["gem"].clone(),
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




#[cfg(test)]
mod tests {
    use super::*;

    // The runtime banner draws only published + obtainable cards, honors the
    // ensured slot, and speaks the agreed lottery_item id/number convention
    #[test]
    fn runtime_custom_banner_draw() {
        let _lock = crate::runtime::lock_test_data_path();
        crate::router::custom_card::tests::wipe(6001);

        // Nothing obtainable: the draw comes back empty (the route then bails
        // before charging)
        assert!(custom_banner_cards(11).is_empty());

        let mut r1_ids = Vec::new();
        for seed in 0..3 {
            let id = custom_card_db::next_card_id();
            custom_card_db::insert_card(id, 1001, 6001, &object!{ "master_card_id": id, "rarity": 1, "seed": seed }, true, true);
            r1_ids.push(id);
        }
        let r2 = custom_card_db::next_card_id();
        custom_card_db::insert_card(r2, 1001, 6001, &object!{ "master_card_id": r2, "rarity": 2 }, true, true);
        let r3 = custom_card_db::next_card_id();
        custom_card_db::insert_card(r3, 1001, 6001, &object!{ "master_card_id": r3, "rarity": 3 }, true, true);
        // Draft / unobtainable cards must never come out of the pool
        let draft = custom_card_db::next_card_id();
        custom_card_db::insert_card(draft, 1001, 6001, &object!{ "master_card_id": draft, "rarity": 1 }, false, true);
        let unobtainable = custom_card_db::next_card_id();
        custom_card_db::insert_card(unobtainable, 1001, 6001, &object!{ "master_card_id": unobtainable, "rarity": 1 }, true, false);

        let drawn = custom_banner_cards(11);
        assert_eq!(drawn.len(), 11);
        // The ensured slot is drawn first: rarity 2 or 3
        let first = drawn[0]["master_card_id"].as_i64().unwrap();
        assert!(first == r2 || first == r3, "ensured slot drew {}", first);
        for card in drawn.members() {
            let id = card["master_card_id"].as_i64().unwrap();
            assert!(r1_ids.contains(&id) || id == r2 || id == r3, "drew {}", id);
            let rarity = custom_card_db::get_card(id).unwrap()["rarity"].as_i64().unwrap();
            // The contract the client synthesizes its LotteryItemMst rows to
            assert_eq!(card["master_lottery_item_id"].as_i64(), Some(690_000_100 + rarity));
            assert_eq!(card["master_lottery_item_number"].as_i64(), Some(id - 150_000_000));
            assert_eq!(card["id"], card["master_card_id"]);
        }

        // Single draws have no ensured slot but still only draw the pool
        let single = custom_banner_cards(1);
        assert_eq!(single.len(), 1);

        // Price rows mirror the baked banners; unknown numbers are refused
        assert_eq!(custom_banner_price(1)["price"].as_i64(), Some(3000));
        assert_eq!(custom_banner_price(1)["count"].as_i64(), Some(11));
        assert_eq!(custom_banner_price(2)["price"].as_i64(), Some(300));
        assert_eq!(custom_banner_price(2)["count"].as_i64(), Some(1));
        assert!(custom_banner_price(3).is_null());

        crate::router::custom_card::tests::wipe(6001);
    }

    #[test]
    fn custom_banner_draw() {
        for lid in [6110001i64, 6110002, 6110003, 6110004] {
            let cards = super::get_random_cards(lid, 11);
            assert_eq!(cards.len(), 11, "banner {lid}");
            for c in cards.members() {
                let id = c["master_card_id"].as_i64().unwrap();
                let rarity = crate::router::items::get_rarity(id);
                assert!((1..=3).contains(&rarity), "card {id} rarity {rarity}");
            }
        }
    }
}
