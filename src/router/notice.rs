use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};

//todo
pub fn reward(_req: HttpRequest) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "reward_list": []
        }
    };
    global::send(resp)
}

pub fn reward_post(_req: HttpRequest, _body: String) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp)
}
