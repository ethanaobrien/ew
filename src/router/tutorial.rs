use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

pub fn tutorial(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    user["tutorial_step"] = body["step"].clone();
    user["stamina"]["stamina"] = (100).into();
    user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    userdata::save_acc(&key, user);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp)
}
