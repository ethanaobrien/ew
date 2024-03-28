use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

pub fn home(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers());
    let user = userdata::get_acc_home(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user
    };
    global::send(resp)
}
