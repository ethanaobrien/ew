use json;
use json::object;
use crate::router::global;
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
