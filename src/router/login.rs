use json;
use json::object;
use crate::router::global;
//use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

//First time login handler
pub fn dummy(req: HttpRequest, _body: String) -> HttpResponse {
    //let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let user = userdata::get_acc(key);
    
    println!("Signin from uid: {}", user["user"]["id"].clone());
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "user_id": user["user"]["id"].clone()
        }
    };
    global::send(resp)
}

pub fn bonus(req: HttpRequest, _body: String) -> HttpResponse {
    //let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let user = userdata::get_acc_home(key);
    
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
