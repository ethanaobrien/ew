use json::{object, array, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{global, userdata, items};
use crate::encryption;

pub fn preset(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc_home(&key);
    
    for (_i, data) in user["home"]["preset_setting"].members_mut().enumerate() {
        if data["slot"].to_string() == body["slot"].to_string() {
            *data = body.clone();
        }
    }
    userdata::save_acc_home(&key, user);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp, req)
}

fn check_gifts(user: &mut JsonValue) {
    let mut to_remove = array![];
    for (j, data) in user["home"]["gift_list"].members().enumerate() {
        if data["is_receive"].to_string() == "1" || data["expire_date_time"].as_u64().unwrap() < global::timestamp() {
            to_remove.push(j).unwrap();
        }
    }
    for (i, data) in to_remove.members().enumerate() {
        user["home"]["gift_list"].array_remove(data.as_usize().unwrap() - i);
    }
}

pub fn gift_get(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let mut user = userdata::get_acc_home(&key);
    check_gifts(&mut user);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "gift_list": user["home"]["gift_list"].clone()
        }
    };
    global::send(resp, req)
}

pub fn preset_get(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "master_preset_background_ids": [1,2,3,4,5],
            "master_preset_foreground_ids": [1,2,3],
            "card_list": user["card_list"].clone()
        }
    };
    global::send(resp, req)
}

pub fn home(req: HttpRequest) -> HttpResponse {
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
    let home_missions = array![];
    
    let mut clear_ct = 0;
    let mut daily_ct = 0;
    for (_i, mission) in user_missions.members().enumerate() {
        if mission["status"].as_i32().unwrap() != 2 {
            continue;
        }
        if home_missions.contains(mission["master_mission_id"].as_i64().unwrap()) {
            clear_ct += 1;
        }
        if daily_missions.contains(mission["master_mission_id"].as_i64().unwrap()) {
            daily_ct += 1;
            clear_ct += 1;
        }
    }
    user["home"]["clear_mission_count"] = clear_ct.into();
    user["home"]["not_cleared_daily_mission_count"] = (6 - daily_ct).into();
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user
    };
    global::send(resp, req)
}
