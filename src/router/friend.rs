use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};
//use crate::router::userdata;

pub fn friend(_req: HttpRequest, _body: String) -> HttpResponse {
    /*let blank_header = HeaderValue::from_static("");
    
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let user = userdata::get_acc(key, uid);*/
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "friend_list": [] //todo - pull from userdata
        }
    };
    global::send(resp)
}
