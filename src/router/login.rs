use json;
use json::object;
use crate::router::global;
//use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

//First time login handler
pub fn dummy(req: HttpRequest, body: String) -> HttpResponse {
    //let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let key = global::get_login(req.headers(), &body);
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "user_id": user["user"]["id"].clone()
        }
    };
    global::send(resp)
}

pub fn bonus(req: HttpRequest, body: String) -> HttpResponse {
    //let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let key = global::get_login(req.headers(), &body);
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "login_bonus_list": [],
            "start_time": global::timestamp(),
            "clear_mission_ids": user["clear_mission_ids"].clone()
        }
    };
    global::send(resp)
}
