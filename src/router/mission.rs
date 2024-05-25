use json::{array, object, JsonValue};
use actix_web::{HttpRequest};

use crate::router::{global, userdata, items, databases};
use crate::encryption;

pub fn mission(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let missions = userdata::get_acc_missions(&key);
    
    Some(object!{
        "mission_list": missions
    })
}

pub fn clear(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    
    let mut missions = userdata::get_acc_missions(&key);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    for id in body["master_mission_ids"].members() {
        items::update_mission_status(id.as_i64().unwrap(), 0, true, true, 1, &mut missions);
    }
    
    userdata::save_acc_missions(&key, missions);
    
    Some(object!{
        "clear_mission_ids": body["master_mission_ids"].clone()
    })
}

pub fn receive(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut missions = userdata::get_acc_missions(&key);
    let mut user = userdata::get_acc(&key);
    let mut rewards = array![];
    
    for mission in body["master_mission_ids"].members() {
        let mission_info = databases::MISSION_LIST[mission.to_string()].clone();
        let mut gift = databases::MISSION_REWARD[mission_info["masterMissionRewardId"].to_string()].clone();
        gift["reward_type"] = gift["type"].clone();
        gift["amount"] = gift["amount"].as_i64().unwrap().into();
        items::give_gift(&gift, &mut user, &mut missions, &mut array![]);
        rewards.push(gift).unwrap();
        
        let variable_missions = array![[1153001, 1153019], [1105001, 1105017], [1101001, 1101030], [1121001, 1121019], [1112001, 1112033]];
        let mut variable = false;
        for id in variable_missions.members() {
            if mission.as_i64().unwrap() >= id[0].as_i64().unwrap() && mission.as_i64().unwrap() < id[1].as_i64().unwrap() {
                items::change_mission_id(mission.as_i64().unwrap(), mission.as_i64().unwrap() + 1, &mut missions);
                items::advance_variable_mission(id[0].as_i64().unwrap(), id[1].as_i64().unwrap(), 0, &mut missions);
                variable = true;
                break;
            }
        }
        if mission.as_i64().unwrap() >= 1158001 && mission.as_i64().unwrap() <= 1158039 {
            items::change_mission_id(mission.as_i64().unwrap(), mission.as_i64().unwrap() + 39, &mut missions);
            items::update_mission_status(mission.as_i64().unwrap() + 39, 0, false, false, 0, &mut missions);
            variable = true;
        }
        if !variable {
            items::update_mission_status(mission.as_i64().unwrap(), 0, true, true, 0, &mut missions);
        }
    }
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, missions.clone());
    
    Some(object!{
        "reward_list": rewards,
        "updated_value_list": {
            "gem": user["gem"].clone(),
            "item_list": user["item_list"].clone(),
            "point_list": user["point_list"].clone()
        },
        "mission_list": missions
    })
}
