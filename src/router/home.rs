use json::{object, array, JsonValue};
use actix_web::{HttpRequest};
use lazy_static::lazy_static;

use crate::router::{global, userdata, items};
use crate::encryption;

pub fn preset(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc_home(&key);
    
    for (_i, data) in user["home"]["preset_setting"].members_mut().enumerate() {
        if data["slot"] == body["slot"] {
            *data = body.clone();
        }
    }
    userdata::save_acc_home(&key, user);
    
    Some(array![])
}

fn check_gifts(user: &mut JsonValue) {
    let mut to_remove = array![];
    for (j, data) in user["home"]["gift_list"].members().enumerate() {
        if data["is_receive"] == 1 || data["expire_date_time"].as_u64().unwrap() < global::timestamp() {
            to_remove.push(j).unwrap();
        }
    }
    for (i, data) in to_remove.members().enumerate() {
        user["home"]["gift_list"].array_remove(data.as_usize().unwrap() - i);
    }
}

pub fn gift_get(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let mut user = userdata::get_acc_home(&key);
    check_gifts(&mut user);
    
    Some(object!{
        "gift_list": user["home"]["gift_list"].clone()
    })
}

pub fn preset_get(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    
    Some(object!{
        "master_preset_background_ids": [1,2,3,4,5],
        "master_preset_foreground_ids": [1,2,3],
        "card_list": user["card_list"].clone()
    })
}


lazy_static! {
    pub static ref HOME_MISSIONS: JsonValue = {
        let mut missions = array![1176001, 1177001, 1177002, 1176002, 1176003, 1176004, 1176005, 1176006, 1169001];
        for i in 1153001..=1153019 {
            missions.push(i).unwrap();
        }
        for i in 1105001..=1105017 {
            missions.push(i).unwrap();
        }
        for i in 1101001..=1101030 {
            missions.push(i).unwrap();
        }
        for i in 1158001..=1158082 {
            missions.push(i).unwrap();
        }
        for i in 1121001..=1121019 {
            missions.push(i).unwrap();
        }
        for i in 1112001..=1112033 {
            missions.push(i).unwrap();
        }
        missions
    };
}

pub fn home(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let mut user = userdata::get_acc_home(&key);
    
    check_gifts(&mut user);
    
    let mut user_missions = userdata::get_acc_missions(&key);
    let clear = items::completed_daily_mission(1253003, &mut user_missions);
    userdata::save_acc_home(&key, user.clone());
    user["clear_mission_ids"] = clear;
    if !user["clear_mission_ids"].is_empty() {
        userdata::save_acc_missions(&key, user_missions.clone());
    }
    
    let daily_missions = array![1224003, 1253003, 1273009, 1273010, 1273011, 1273012];
    
    let mut clear_ct = 0;
    let mut daily_ct = 0;
    for (_i, mission) in user_missions.members().enumerate() {
        if mission["status"].as_i32().unwrap() != 2 {
            continue;
        }
        if HOME_MISSIONS.contains(mission["master_mission_id"].as_i64().unwrap()) {
            clear_ct += 1;
        }
        if daily_missions.contains(mission["master_mission_id"].as_i64().unwrap()) {
            daily_ct += 1;
            clear_ct += 1;
        }
    }
    user["home"]["clear_mission_count"] = clear_ct.into();
    user["home"]["not_cleared_daily_mission_count"] = (6 - daily_ct).into();
    
    Some(user)
}
