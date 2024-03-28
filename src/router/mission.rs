use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

pub fn mission(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers());
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "mission_list": user["live_mission_list"].clone()
        }
    };
    global::send(resp)
}
