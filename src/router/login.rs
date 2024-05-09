use json::{object, array, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{global, userdata, items, databases};

pub fn dummy(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "user_id": user["user"]["id"].clone()
        }
    };
    global::send(resp, req)
}

pub fn get_login_bonus_info(id: i64) -> JsonValue {
    databases::LOTTERY_INFO[id.to_string()].clone()
}

fn do_bonus(user_home: &mut JsonValue, bonuses: &mut JsonValue) -> JsonValue {
    let last_reset = global::timestamp_since_midnight();
    let to_send;
    if bonuses["last_rewarded"].as_u64().unwrap() < last_reset {
        let mut to_rm = array![];
        for (i, data) in bonuses["bonus_list"].members_mut().enumerate() {
            let info = get_login_bonus_info(data["master_login_bonus_id"].as_i64().unwrap());
            let mut current = data["day_counts"].len();
            if current >= info["days"].len() && info["info"]["loop"].as_i32().unwrap_or(0) == 1 {
                data["day_counts"] = array![];
                current = 0;
            } else if current >= info["days"].len() {
                to_rm.push(i).unwrap();
                continue;
            }
            let item_id = &databases::LOGIN_REWARDS[info["days"][current]["masterLoginBonusRewardId"].to_string()];
            
            items::gift_item(item_id, &format!("Event login bonus day {}!", current+1), user_home);
            data["day_counts"].push(current + 1).unwrap();
        }
        for (i, data) in to_rm.members().enumerate() {
            bonuses["bonus_list"].array_remove(data.as_usize().unwrap() - i);
        }
        bonuses["last_rewarded"] = last_reset.into();
        to_send = bonuses["bonus_list"].clone();
    } else {
        to_send = array![];
    }
    to_send
}

pub fn bonus(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut user_home = userdata::get_acc_home(&key);
    let mut user_missions = userdata::get_acc_missions(&key);
    
    let mut bonuses = userdata::get_acc_loginbonus(&key);
    if bonuses["bonus_list"].is_empty() {
        global::start_login_bonus(1, &mut bonuses);
    }
    let to_send = do_bonus(&mut user_home, &mut bonuses);
    
    let mut cleared_missions = array![];
    if !to_send.is_empty() {
        cleared_missions = items::completed_daily_mission(1253003, &mut user_missions);
        userdata::save_acc_missions(&key, user_missions);
    }
    
    userdata::save_acc_loginbonus(&key, bonuses.clone());
    userdata::save_acc_home(&key, user_home.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "login_bonus_list": to_send,
            "start_time": bonuses["start_time"].clone(),
            "clear_mission_ids": cleared_missions
        }
    };
    global::send(resp, req)
}

pub fn bonus_event(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut user_home = userdata::get_acc_home(&key);
    
    let mut bonuses = userdata::get_acc_eventlogin(&key);
    if bonuses["bonus_list"].is_empty() {
        global::start_login_bonus(20039, &mut bonuses);
    }
    let to_send = do_bonus(&mut user_home, &mut bonuses);
    
    userdata::save_acc_eventlogin(&key, bonuses.clone());
    userdata::save_acc_home(&key, user_home.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "login_bonus_list": to_send,
            "start_time": bonuses["start_time"].clone(),
            "clear_mission_ids": []
        }
    };
    global::send(resp, req)
}
