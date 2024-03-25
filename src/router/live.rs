use json;
use json::object;
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue};
use crate::router::userdata;

pub fn retire(_req: HttpRequest, _body: String) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "stamina": {},
            "item_list": [],
            "event_point_list": []
        }
    };
    global::send(resp)
}

pub fn guest(_req: HttpRequest, _body: String) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {"guest_list":[{"user":{"name":"A Nice Guest","comment":"Enjoy the first live show!","exp":900,"main_deck_slot":1,"favorite_master_card_id":10010013,"favorite_card_evolve":0,"guest_smile_master_card_id":10010013,"guest_cool_master_card_id":10010013,"guest_pure_master_card_id":10010013,"friend_request_disabled":1,"master_title_ids":[3000001,0],"profile_settings":[1,2,3,4,5,6,7],"last_login_time":1708699449},"favorite_card":{"id":0,"master_card_id":10010013,"exp":1025,"skill_exp":0,"evolve":[]},"guest_smile_card":{"id":0,"master_card_id":10010013,"exp":1025,"skill_exp":0,"evolve":[]},"guest_cool_card":{"id":0,"master_card_id":10010013,"exp":1025,"skill_exp":0,"evolve":[]},"guest_pure_card":{"id":0,"master_card_id":10010013,"exp":1025,"skill_exp":0,"evolve":[]},"status":0}]}
    };
    global::send(resp)
}

pub fn start(_req: HttpRequest, _body: String) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp)
}

pub fn end(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let blank_header = HeaderValue::from_static("");
    let key = req.headers().get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let user2 = userdata::get_acc_home(key);
    let mut user = userdata::get_acc(key);
    
    user["stamina"]["stamina"] = (user["stamina"]["stamina"].as_i32().unwrap() - body["use_lp"].as_i32().unwrap()).into();
    if user["stamina"]["stamina"].as_i32().unwrap() < 0 {
        user["stamina"]["stamina"] = (0).into();
    }
    user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    userdata::save_acc(key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "item_list": user["item_list"].clone(),
            "point_list": user["point_list"].clone(),
            "live": null, //likely a server bug?
            "clear_master_live_mission_ids": [],
            "user": user["user"].clone(),
            "stamina": user["stamina"].clone(),
            "character_list": user["character_list"].clone(),
            "reward_list": [],
            "gift_list": user2["home"]["gift_list"].clone(),
            "clear_mission_ids": user2["clear_mission_ids"].clone(),
            "event_point_reward_list": [],
            "ranking_change": [],
            "event_member": [],
            "event_ranking_data": []
        }
    };
    global::send(resp)
}

pub fn clearrate(_req: HttpRequest) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": json::parse(include_str!("clearrate.json")).unwrap()
    };
    global::send(resp)
}
