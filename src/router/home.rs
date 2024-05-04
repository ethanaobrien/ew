use json::{object, array, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::userdata;
use crate::router::global;
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
    global::send(resp)
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
    global::send(resp)
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
    global::send(resp)
}

pub fn home(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let mut user = userdata::get_acc_home(&key);
    
    check_gifts(&mut user);
    userdata::save_acc_home(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user
    };
    global::send(resp)
}
