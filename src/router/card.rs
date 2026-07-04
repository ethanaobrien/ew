use jzon::{object, array, JsonValue};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{userdata, global, items, databases};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/card")
            .route("/reinforce", web::post().to(reinforce))
            .route("/skill/reinforce", web::post().to(skill_reinforce))
            .route("/evolve", web::post().to(evolve))
    );
}

fn exp_cap(master_card_id: i64, evolved: bool) -> i64 {
    let card = &databases::CARD_LIST[master_card_id.to_string()];
    let rarity = card["rarity"].to_string();
    let curve = card["masterCardLevelId"].as_i64().unwrap_or(0);
    let max_level = if evolved {
        databases::CARD_EVOLVE[&rarity]["maxLevel"].as_i64().unwrap_or(0)
    } else {
        databases::CARD_RARITY[&rarity]["maxLevel"].as_i64().unwrap_or(0)
    };
    databases::CARD_LEVEL[format!("{}_{}", curve, max_level)].as_i64().unwrap_or(i64::MAX)
}

fn skill_exp_cap(master_card_id: i64) -> i64 {
    let card = &databases::CARD_LIST[master_card_id.to_string()];
    let rarity = card["rarity"].to_string();
    let skill_curve = databases::CARD_RARITY[&rarity]["masterCardSkillLevelId"].to_string();
    databases::CARD_SKILL_MAX[skill_curve].as_i64().unwrap_or(i64::MAX)
}

fn material_item_list(user: &JsonValue, body: &JsonValue) -> JsonValue {
    let mut rv = array![];
    for mat in body["material_item_list"].members() {
        let mid = mat["master_item_id"].as_i64();
        for it in user["item_list"].members() {
            if it["master_item_id"].as_i64() == mid {
                rv.push(it.clone()).unwrap();
                break;
            }
        }
    }
    rv
}

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
            
            if exp_id == "exp" {
                let cap = exp_cap(card["master_card_id"].as_i64().unwrap(), !card["evolve"].is_empty());
                if card["exp"].as_i64().unwrap() > cap {
                    card["exp"] = cap.into();
                }
            } else if exp_id == "skill_exp" {
                let cap = skill_exp_cap(card["master_card_id"].as_i64().unwrap());
                if card["skill_exp"].as_i64().unwrap() > cap {
                    card["skill_exp"] = cap.into();
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

async fn reinforce(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut clear_mission_ids = array![];
    
    let card = do_reinforce(&mut user, &body, "exp", 1, false, &mut array![], &mut array![], &mut clear_mission_ids);

    userdata::save_acc(&key, user.clone());

    global::api(&req, Some(object!{
        card: card,
        item_list: material_item_list(&user, &body),
        point_list: user["point_list"].clone(),
        clear_mission_ids: clear_mission_ids
    }))
}

async fn skill_reinforce(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut clear_mission_ids = array![];
    
    let card = do_reinforce(&mut user, &body, "skill_exp", 10, false, &mut array![], &mut array![], &mut clear_mission_ids);

    userdata::save_acc(&key, user.clone());

    global::api(&req, Some(object!{
        card: card,
        item_list: material_item_list(&user, &body),
        point_list: user["point_list"].clone(),
        clear_mission_ids: clear_mission_ids
    }))
}

async fn evolve(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut clear_mission_ids = array![];
    
    let card = do_reinforce(&mut user, &body, "", 0, true, &mut missions, &mut chats, &mut clear_mission_ids);
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_missions(&key, missions);

    global::api(&req, Some(object!{
        card: card,
        item_list: material_item_list(&user, &body),
        point_list: user["point_list"].clone(),
        clear_mission_ids: clear_mission_ids,
        gift_list: array![],
        updated_value_list: array![]
    }))
}
