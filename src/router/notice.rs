use json::object;
use actix_web::{HttpResponse, HttpRequest};

use crate::router::global;

//todo
pub fn reward(req: HttpRequest) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "reward_list": []
        }
    };
    global::send(resp, req)
}

pub fn reward_post(req: HttpRequest, _body: String) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp, req)
}
