use json::{object, array, JsonValue};
use actix_web::{HttpRequest};

use crate::router::{userdata, global, items, databases};
use crate::encryption;

fn do_reinforce(user: &mut JsonValue, body: &JsonValue, exp_id: &str, money_multiplier: i64, evolve: bool) -> JsonValue {
    for (i, data) in user["card_list"].members().enumerate() {
        if data["id"] == body["id"] {
            let materials = &body["material_item_list"];
            let mut card = data.clone();
            let mut money: i64 = 0;
            
            for data2 in materials.members() {
                items::use_item(&object!{
                    value: data2["master_item_id"].as_i64().unwrap(),
                    amount: 1,
                    consumeType: 4
                }, data2["amount"].as_i64().unwrap(), user);
                let item = &databases::ITEM_INFO[data2["master_item_id"].to_string()];
                if evolve {
                    card["evolve"] = array![{type: 2,count: 1}];
                    money = databases::EVOLVE_COST[items::get_rarity(card["master_card_id"].as_i64().unwrap()).to_string()].as_i64().unwrap();
                } else {
                    card[exp_id] = (card[exp_id].as_i64().unwrap() + (item["effectValue"].as_i64().unwrap() * data2["amount"].as_i64().unwrap())).into();
                    money += item["effectValue"].as_i64().unwrap() * data2["amount"].as_i64().unwrap() * money_multiplier;
                }
            }
            
            user["card_list"][i] = card.clone();

            for data in user["point_list"].members_mut() {
                if data["type"].as_i32().unwrap() == 1 {
                    data["amount"] = (data["amount"].as_i64().unwrap() - money).into();
                }
            }
            return card;
        }
    }
    object!{}
}

pub fn reinforce(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    let card = do_reinforce(&mut user, &body, "exp", 1, false);
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        card: card,
        item_list: user["item_list"].clone(),
        point_list: user["point_list"].clone(),
        clear_mission_ids: []
    })
}

pub fn skill_reinforce(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    let card = do_reinforce(&mut user, &body, "skill_exp", 10, false);
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        card: card,
        item_list: user["item_list"].clone(),
        point_list: user["point_list"].clone(),
        clear_mission_ids: []
    })
}

pub fn evolve(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    let card = do_reinforce(&mut user, &body, "", 0, true);
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        card: card,
        item_list: user["item_list"].clone(),
        point_list: user["point_list"].clone(),
        clear_mission_ids: []
    })
}
