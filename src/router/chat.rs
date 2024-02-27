use json;
use json::object;
use crate::router::global;
//use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

pub fn home(req: HttpRequest, _body: String) -> HttpResponse {
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let user = userdata::get_acc(key, uid);
    
    let id = user["user"]["favorite_master_card_id"].as_i64().unwrap() / 10000;
    
    let chapter_id = (id * 100000) + 101;
    let room_id = (id * 1000) + 1;
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "progress_list": [
                {
                    "chat_id": id,
                    "room_id": 1,
                    "chapter_id": chapter_id,
                    "is_read": 0,
                    "created_at": global::timestamp()
                }
            ],
            "master_chat_room_ids": [room_id],
            "master_chat_stamp_ids": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,43,44,45,46,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,11001003,22001001,33001001,44001002],
            "master_chat_attachment_ids": []
        }
    };
    global::send(resp)
}

pub fn start(_req: HttpRequest, _body: String) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {"select_talk_id_list":[],"get_item_list":[],"is_read":0}
    };
    global::send(resp)
}

pub fn end(_req: HttpRequest, _body: String) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp)
}
