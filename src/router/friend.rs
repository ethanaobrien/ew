use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};
//use crate::router::userdata;

pub fn friend(_req: HttpRequest, _body: String) -> HttpResponse {
    /*let blank_header = HeaderValue::from_static("");
    
    let key = global::get_login(req.headers());
    let user = userdata::get_acc(&key, uid);*/
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "friend_list": [] //todo - pull from userdata
        }
    };
    global::send(resp)
}
