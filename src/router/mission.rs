use json::{array, object, JsonValue};
use actix_web::{HttpResponse, HttpRequest};
use lazy_static::lazy_static;

use crate::router::{global, userdata};
use crate::encryption;
use crate::router::items;

pub fn mission(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let missions = userdata::get_acc_missions(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "mission_list": missions
        }
    };
    global::send(resp, req)
}

pub fn clear(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut missions = userdata::get_acc_missions(&key);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    for (_i, id) in body["master_mission_ids"].members().enumerate() {
        for (i, mission) in missions.members().enumerate() {
            if mission["master_mission_id"].to_string() == id.to_string() {
                //I think this is all?
                missions[i]["progress"] = (1).into();
                break;
            }
        }
    }
    
    userdata::save_acc_missions(&key, missions);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "clear_mission_ids": body["master_mission_ids"].clone()
        }
    };
    global::send(resp, req)
}

lazy_static! {
    static ref MISSION_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/mission.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    static ref MISSION_REWARD: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/mission_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
}

pub fn receive(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut missions = userdata::get_acc_missions(&key);
    let mut user = userdata::get_acc(&key);
    let mut rewards = array![];
    
    for (_i, mission) in body["master_mission_ids"].members().enumerate() {
        items::update_mission_status(mission.as_i64().unwrap(), 0, true, true, false, &mut missions);
        let mission_info = MISSION_LIST[mission.to_string()].clone();
        let mut gift = MISSION_REWARD[mission_info["masterMissionRewardId"].to_string()].clone();
        gift["reward_type"] = gift["type"].clone();
        gift["amount"] = gift["amount"].as_i64().unwrap().into();
        items::give_gift(&gift, &mut user);
        rewards.push(gift).unwrap();
    }
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, missions.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "reward_list": rewards,
            "updated_value_list": {
                "gem": user["gem"].clone(),
                "item_list": user["item_list"].clone(),
                "point_list": user["point_list"].clone()
            },
            "mission_list": missions
            
        }
    };
    global::send(resp, req)
}
