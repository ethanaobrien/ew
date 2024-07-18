use json::{JsonValue, object};
use actix_web::HttpRequest;
use rand::Rng;

use crate::encryption;
use crate::include_file;
use crate::router::{userdata, global, databases};

fn get_event_data(key: &str, event_id: i64) -> JsonValue {
    let mut event = userdata::get_acc_event(key);

    if event[event_id.to_string()].is_empty() {
        event[event_id.to_string()] = json::parse(&include_file!("src/router/userdata/new_user_event.json")).unwrap();
        let mut ev = event[event_id.to_string()].clone();
        init_star_event(&mut ev);
        save_event_data(key, event_id, ev);
        event = userdata::get_acc_event(key);
    }
    event[event_id.to_string()].clone()
}

fn save_event_data(key: &str, event_id: i64, data: JsonValue) {
    let mut event = userdata::get_acc_event(key);

    // Check for old version of event data
    if !event["event_data"].is_empty() {
        event = object!{};
    }

    event[event_id.to_string()] = data;

    userdata::save_acc_event(key, event);
}

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
    for (j, live) in event["star_event"]["star_music_list"].members().enumerate() {
        if live["position"] == index {
            i = j;
            break;
        }
    }
    if i != 0 {
        event["star_event"]["star_music_list"].array_remove(i);
    }

    let random_song = get_random_song();
    let to_push = object!{
        master_music_id: random_song["song"].clone(),
        position: index,
        is_cleared: 0,
        goal_score: random_song["score"].clone()
    };
    event["star_event"]["star_music_list"].push(to_push).unwrap();
}

fn init_star_event(event: &mut JsonValue) {
    if event["star_event"]["star_level"].as_i32().unwrap() != 0 {
        return;
    }
    event["star_event"]["star_level"] = 1.into();
    switch_music(event, 1);
    switch_music(event, 2);
    switch_music(event, 3);
    switch_music(event, 4);
    switch_music(event, 5);
}

pub fn event(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let event = get_event_data(&key, body["master_event_id"].as_i64().unwrap());

    Some(event)
}

pub fn star_event(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();

    let event = get_event_data(&key, body["master_event_id"].as_i64().unwrap());

    Some(object!{
        star_event: event["star_event"].clone(),
        gift_list: [],
        reward_list: []
    })
}

pub fn change_target_music(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = &encryption::decrypt_packet(&body).unwrap();
    let body: StarEventChangeTargetMusic = serde_json::from_str(body).unwrap();

    let mut event = get_event_data(&key, body.master_event_id as i64);

    event["star_event"]["music_change_count"] = (event["star_event"]["music_change_count"].as_i32().unwrap() + 1).into();

    switch_music(&mut event, body.position as i32);

    save_event_data(&key, body.master_event_id as i64, event.clone());

    Some(event["star_event"].clone())
}

pub fn set_member(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut event = get_event_data(&key, body["master_event_id"].as_i64().unwrap());

    event["member_ranking"] = object!{
        master_character_id: body["master_character_id"].clone(),
        rank: 0,
        point: 0
    };

    save_event_data(&key, body["master_event_id"].as_i64().unwrap(), event.clone());

    Some(object!{
        event_member: event["member_ranking"].clone()
    })
}

pub fn ranking(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{
        ranking_detail_list: []
    })
}

// Start request structs

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct StarEventChangeTargetMusic {
    master_event_id: usize,
    position: usize
}
