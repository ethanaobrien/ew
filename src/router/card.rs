use json::{object, array, JsonValue};
use actix_web::{HttpRequest};

use crate::router::{userdata, global, items, databases};
use crate::encryption;

// Chats will only ever be used when evolving
fn do_reinforce(user: &mut JsonValue, body: &JsonValue, exp_id: &str, money_multiplier: i64, evolve: bool, missions: &mut JsonValue, chats: &mut JsonValue, clear_mission_ids: &mut JsonValue) -> JsonValue {
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
            if evolve && !databases::CHARACTER_CHATS[card["master_card_id"].to_string()]["50"].is_empty() {
                let chat = &databases::CHARACTER_CHATS[card["master_card_id"].to_string()]["50"];
                let mission_id = databases::MISSION_REWARD[chat[0].to_string()]["value"].as_i64().unwrap();

                if crate::router::chat::add_chat_from_chapter_id(mission_id, chats) {
                    items::update_mission_status(chat[1].as_i64().unwrap(), 0, true, true, 1, missions);
                    if !clear_mission_ids.contains(chat[1].as_i64().unwrap()) {
                        clear_mission_ids.push(chat[1].clone()).unwrap();
                    }
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
    let mut clear_mission_ids = array![];
    
    let card = do_reinforce(&mut user, &body, "exp", 1, false, &mut array![], &mut array![], &mut clear_mission_ids);
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        card: card,
        item_list: user["item_list"].clone(),
        point_list: user["point_list"].clone(),
        clear_mission_ids: clear_mission_ids
    })
}

pub fn skill_reinforce(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut clear_mission_ids = array![];
    
    let card = do_reinforce(&mut user, &body, "skill_exp", 10, false, &mut array![], &mut array![], &mut clear_mission_ids);
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        card: card,
        item_list: user["item_list"].clone(),
        point_list: user["point_list"].clone(),
        clear_mission_ids: clear_mission_ids
    })
}

pub fn evolve(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut clear_mission_ids = array![];
    
    let card = do_reinforce(&mut user, &body, "", 0, true, &mut missions, &mut chats, &mut clear_mission_ids);
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_missions(&key, missions);
    
    Some(object!{
        card: card,
        item_list: user["item_list"].clone(),
        point_list: user["point_list"].clone(),
        clear_mission_ids: clear_mission_ids
    })
}
