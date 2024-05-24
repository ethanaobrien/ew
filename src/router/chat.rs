use json::{object, array, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{global, items, userdata, databases};
use crate::encryption;

pub fn add_chat(id: i64, num: i64, chats: &mut JsonValue) {
    chats.push(object!{
        chat_id: id,
        room_id: num,
        chapter_id: databases::CHAPTERS[id.to_string()][num.to_string()]["id"].clone(),
        is_read: 0,
        created_at: global::timestamp()
    }).unwrap();
}

pub fn home(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let chats = userdata::get_acc_chats(&key);
    
    let mut rooms = array![];
    for (_i, data) in chats.members().enumerate() {
        rooms.push(databases::CHATS[data["chat_id"].to_string()][data["room_id"].to_string()]["id"].clone()).unwrap();
    }
    
    Some(object!{
        "progress_list": chats,
        "master_chat_room_ids": rooms,
        "master_chat_stamp_ids": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,43,44,45,46,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,11001003,22001001,33001001,44001002],
        "master_chat_attachment_ids": []
    })
}

pub fn start(req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{"select_talk_id_list":[],"get_item_list":[],"is_read":0})
}

pub fn end(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    
    for (_i, data) in chats.members_mut().enumerate() {
        if body["chapter_id"].as_i64().unwrap() == data["chapter_id"].as_i64().unwrap() {
            if data["is_read"].as_i32().unwrap() != 1 {
                items::advance_mission(1169001, 1, 50, &mut missions);
            }
            data["is_read"] = 1.into();
            userdata::save_acc_missions(&key, missions);
            userdata::save_acc_chats(&key, chats);
            break;
        }
    }
    
    Some(array![])
}
