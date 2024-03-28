use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

pub fn tutorial(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let key = global::get_login(req.headers());
    let mut user = userdata::get_acc(&key);
    
    user["tutorial_step"] = body["step"].clone();
    
    userdata::save_acc(&key, user);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp)
}
