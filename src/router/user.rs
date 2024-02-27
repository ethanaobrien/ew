use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

pub fn user(req: HttpRequest) -> HttpResponse {
    let blank_header = HeaderValue::from_static("");
    
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let user = userdata::get_acc(key, uid);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user
    };
    global::send(resp)
}

pub fn user_post(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let blank_header = HeaderValue::from_static("");
    
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let mut user = userdata::get_acc(key, uid);
    let user_2 = userdata::get_acc_home(key, uid);
    
    user["user"]["name"] = body["name"].clone();
    user["user"]["friend_request_disabled"] = body["friend_request_disabled"].clone();
    
    userdata::save_acc(key, uid, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "user": user["user"].clone(),
            "clear_mission_ids": user_2["clear_mission_ids"].clone()
        }
    };
    global::send(resp)
}

pub fn initialize(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let blank_header = HeaderValue::from_static("");
    
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let mut user = userdata::get_acc(key, uid);
    
    let id = (body["master_character_id"].as_i32().unwrap() * 10000) + 7; //todo - is this alwasy the case?
    user["user"]["favorite_master_card_id"] = id.into();
    user["user"]["guest_smile_master_card_id"] = id.into();
    user["user"]["guest_cool_master_card_id"] = id.into();
    user["user"]["guest_pure_master_card_id"] = id.into();
    
    let id = body["master_character_id"].to_string();
    let userr = &id[id.len() - 2..].parse::<i32>().unwrap();
    let mut masterid = 3000000;
    if id.starts_with("2") {
        masterid += 9; //muse
    } else if id.starts_with("3") {
        masterid += 9 + 9; //aquors
    } else if id.starts_with("4") {
        masterid += 9 + 9 + 12; //nijigasaki
    }
    masterid += userr;
    
    user["user"]["master_title_ids"][0] = masterid.into();
    
    userdata::save_acc(key, uid, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user["user"].clone()
    };
    global::send(resp)
}
