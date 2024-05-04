use crate::encryption;
use json::object;
use crate::router::global;
use crate::router::userdata;
use actix_web::{HttpResponse, HttpRequest};

pub fn events(_req: HttpRequest) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "serial_code_list": []
        }
    };
    global::send(resp)
}

pub fn serial_code(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc_home(&key);
    
    let item;
    if body["input_code"].to_string() == "SIF2REVIVALREAL!" {
        item = global::gift_item_basic(1, 100000, 4, "Another game died... This makes me sad :(", &mut user);
    } else if body["input_code"].to_string() == "pweasegivegems11" {
        item = global::gift_item_basic(1, 6000, 1, "Only because you asked...", &mut user);
    } else if body["input_code"].to_string() == "sleepysleepyslep" {
        item = global::gift_item_basic(15540001, 50, 3, "I am tired", &mut user);
    } else {
        let resp = object!{
            "code": 0,
            "server_time": global::timestamp(),
            "data": {
                "result_code": 3
            }
        };
        return global::send(resp);
    }
    
    userdata::save_acc_home(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "serial_code_event": {"id":42,"name":"Serial Code Reward","unique_limit_count":0,"min_user_rank":0,"max_user_rank":0,"end_date":null},
            "reward_list": [item],
            "result_code": 0,
            "gift_list": user["gift_list"].clone(),
            "excluded_gift_list": []
        }
    };
    global::send(resp)
}
