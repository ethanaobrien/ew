use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/mission")
            .route("", web::get().to(mission))
            .route("/clear", web::post().to(clear))
            .route("/receive", web::post().to(receive))
    );
}

const VARIABLE_MISSIONS: [[i64; 2]; 5] = [[1153001, 1153019], [1105001, 1105017], [1101001, 1101030], [1121001, 1121019], [1112001, 1112033]];

async fn mission(req: HttpRequest) -> impl Responder {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    let step = user["tutorial_step"].as_i64().unwrap_or(0);

    if (1..130).contains(&step) {
        return global::api(&req, Some(object!{
            "mission_list": array![]
        }));
    }

    let mut missions = userdata::get_acc_missions(&key);
    if items::refresh_dailies(&mut missions, global::timestamp()) {
        userdata::save_acc_missions(&key, missions.clone());
    }

    global::api(&req, Some(object!{
        "mission_list": missions
    }))
}

async fn clear(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);

    let mut missions = userdata::get_acc_missions(&key);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();

    let mut cleared = array![];
    for id in body["master_mission_ids"].members() {
        let mission = items::get_mission_status(id.as_i64().unwrap(), &missions);
        if mission.is_empty() || mission["status"].as_i32().unwrap_or(0) >= 2 {
            continue;
        }
        items::update_mission_status(id.as_i64().unwrap(), 0, true, false, 1, &mut missions);
        cleared.push(id.clone()).unwrap();
    }

    userdata::save_acc_missions(&key, missions);

    global::api(&req, Some(object!{
        "clear_mission_ids": cleared
    }))
}

async fn receive(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let uid = global::get_uid(req.headers());
    let now = global::timestamp();

    let mut missions = userdata::get_acc_missions(&key);
    let mut user = userdata::get_acc(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut rewards = array![];
    let mut mission_list = array![];
    let mut touched_gem = false;
    let mut touched_coin = false;
    let mut touched_items = array![];

    for mission in body["master_mission_ids"].members() {
        let mid = mission.as_i64().unwrap();
        let mission_info = databases::MISSION_LIST[mid.to_string()].clone();
        let master = databases::MISSION_REWARD[mission_info["masterMissionRewardId"].to_string()].clone();
        let reward_type = master["type"].as_i64().unwrap();

        rewards.push(object!{
            give_type: master["giveType"].clone(),
            type: master["type"].clone(),
            value: master["value"].clone(),
            level: master["level"].clone(),
            amount: master["amount"].clone()
        }).unwrap();

        items::give_gift(&object!{
            reward_type: reward_type,
            value: master["value"].clone(),
            amount: master["amount"].clone()
        }, &mut user, &mut missions, &mut array![], &mut chats);

        match reward_type {
            1 => touched_gem = true,
            4 => touched_coin = true,
            _ => { touched_items.push(master["value"].clone()).unwrap(); }
        }

        let mut variable = false;
        for range in VARIABLE_MISSIONS {
            if mid >= range[0] && mid < range[1] {
                items::change_mission_id(mid, mid + 1, &mut missions);
                let next = databases::MISSION_LIST[(mid + 1).to_string()]["conditionNumber"].as_i64().unwrap_or(0);
                for m in missions.members_mut() {
                    if m["master_mission_id"].as_i64() == Some(mid + 1) {
                        let progress = m["progress"].as_i64().unwrap_or(0);
                        m["status"] = if progress >= next { 2 } else { 1 }.into();
                        mission_list.push(m.clone()).unwrap();
                        break;
                    }
                }
                variable = true;
                break;
            }
        }
        if !variable && (1158001..=1158039).contains(&mid) {
            items::change_mission_id(mid, mid + 39, &mut missions);
            items::update_mission_status(mid + 39, 0, false, false, 0, &mut missions);
            for m in missions.members_mut() {
                if m["master_mission_id"].as_i64() == Some(mid + 39) {
                    mission_list.push(m.clone()).unwrap();
                    break;
                }
            }
            variable = true;
        }
        if !variable {
            for m in missions.members_mut() {
                if m["master_mission_id"].as_i64() == Some(mid) {
                    m["status"] = (3).into();
                    break;
                }
            }
            let m = items::get_mission_status(mid, &missions);
            let expire = m["expire_date_time"].as_u64().unwrap_or(0);
            mission_list.push(object!{
                user_id: uid,
                master_mission_id: mid,
                status: 3,
                progress: if expire != 0 { m["progress"].clone() } else { JsonValue::Null },
                expire_date: if expire != 0 { global::format_datetime(expire).into() } else { JsonValue::Null },
                received_date: global::format_datetime(now)
            }).unwrap();
        }
    }

    userdata::save_acc(&key, user.clone());
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_missions(&key, missions);

    let mut updated_value_list = object!{};
    if touched_gem {
        updated_value_list["gem"] = user["gem"].clone();
    }
    if !touched_items.is_empty() {
        let mut item_list = array![];
        for id in touched_items.members() {
            for item in user["item_list"].members() {
                if item["master_item_id"] == *id {
                    item_list.push(item.clone()).unwrap();
                    break;
                }
            }
        }
        updated_value_list["item_list"] = item_list;
    }
    if touched_coin {
        let mut point_list = array![];
        for point in user["point_list"].members() {
            if point["type"].as_i64() == Some(1) {
                point_list.push(object!{
                    type: 1,
                    amount: point["amount"].clone()
                }).unwrap();
                break;
            }
        }
        updated_value_list["point_list"] = point_list;
    }

    global::api(&req, Some(object!{
        "reward_list": rewards,
        "gift_list": array![],
        "updated_value_list": updated_value_list,
        "mission_list": mission_list
    }))
}
