use json::{JsonValue, object};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{userdata, global};

pub fn event(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut event = userdata::get_acc_event(&key);
    
    init_star_event(&mut event);
    
    userdata::save_acc_event(&key, event.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": event["event_data"].clone()
    };
    global::send(resp, req)
}

fn switch_music(event: &mut JsonValue, music_id: i32, target_score: i64, index: i32) {
    if index <= 4 {
        //todo
    }
    let to_push = object!{
        master_music_id: music_id,
        position: event["event_data"]["star_event"]["star_music_list"].len(),
        is_cleared: 0,
        goal_score: target_score
    };
    event["event_data"]["star_event"]["star_music_list"].push(to_push).unwrap();
}

fn init_star_event(event: &mut JsonValue) {
    if event["event_data"]["star_event"]["star_level"].as_i32().unwrap() != 0 {
        return;
    }
    event["event_data"]["star_event"]["star_level"] = 1.into();
    switch_music(event, 1014, 53407, 5);
    switch_music(event, 2101, 34557, 5);
    switch_music(event, 2120, 38222, 5);
    switch_music(event, 2151, 46076, 5);
    switch_music(event, 2160, 21991, 5);
}

pub fn star_event(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut event = userdata::get_acc_event(&key);
    init_star_event(&mut event);
    
    userdata::save_acc_event(&key, event.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            star_event: event["event_data"]["star_event"].clone(),
            gift_list: [],
            reward_list: []
        }
    };
    global::send(resp, req)
}

//todo - randomize
pub fn change_target_music(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let event = userdata::get_acc_event(&key);
    
    //event["star_event"]["music_change_count"] += 1;
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": event["event_data"]["star_event"].clone()
    };
    global::send(resp, req)
}
