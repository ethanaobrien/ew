use json::{JsonValue, object};
use actix_web::HttpRequest;
use rand::Rng;

use crate::encryption;
use crate::router::{userdata, global, databases};

fn get_random_song() -> JsonValue {
    let mut rng = rand::thread_rng();
    let random_number = rng.gen_range(0..=databases::LIVES.len());
    object!{
        song: databases::LIVES[random_number]["masterMusicId"].clone(),
        score: (databases::LIVES[random_number]["scoreC"].as_f64().unwrap() * 1.75).round() as i64
    }
}

fn switch_music(event: &mut JsonValue, index: i32) {
    if index > 5 || index < 1 {
        return;
    }

    let mut i = 0;
    for (j, live) in event["event_data"]["star_event"]["star_music_list"].members().enumerate() {
        if live["position"] == index {
            i = j;
            break;
        }
    }
    if i != 0 {
        event["event_data"]["star_event"]["star_music_list"].array_remove(i);
    }

    let random_song = get_random_song();
    let to_push = object!{
        master_music_id: random_song["song"].clone(),
        position: index,
        is_cleared: 0,
        goal_score: random_song["score"].clone()
    };
    event["event_data"]["star_event"]["star_music_list"].push(to_push).unwrap();
}

fn init_star_event(event: &mut JsonValue) {
    if event["event_data"]["star_event"]["star_level"].as_i32().unwrap() != 0 {
        return;
    }
    event["event_data"]["star_event"]["star_level"] = 1.into();
    switch_music(event, 1);
    switch_music(event, 2);
    switch_music(event, 3);
    switch_music(event, 4);
    switch_music(event, 5);
}

pub fn event(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let mut event = userdata::get_acc_event(&key);
    
    init_star_event(&mut event);
    
    userdata::save_acc_event(&key, event.clone());
    
    Some(event["event_data"].clone())
}

pub fn star_event(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let mut event = userdata::get_acc_event(&key);
    init_star_event(&mut event);
    
    userdata::save_acc_event(&key, event.clone());
    
    Some(object!{
        star_event: event["event_data"]["star_event"].clone(),
        gift_list: [],
        reward_list: []
    })
}

//todo - randomize
pub fn change_target_music(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut event = userdata::get_acc_event(&key);
    
    event["event_data"]["star_event"]["music_change_count"] = (event["event_data"]["star_event"]["music_change_count"].as_i32().unwrap() + 1).into();

    switch_music(&mut event, body["position"].as_i32().unwrap());

    userdata::save_acc_event(&key, event.clone());
    
    Some(event["event_data"]["star_event"].clone())
}
