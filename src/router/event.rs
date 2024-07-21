use json::{JsonValue, object, array};
use actix_web::HttpRequest;
use rand::Rng;

use crate::encryption;
use crate::include_file;
use crate::router::{userdata, global, databases};

// I believe(?) this is all?
const STAR_EVENT_IDS: [u32; 3] = [127, 135, 139];

fn get_event_data(key: &str, event_id: u32) -> JsonValue {
    let mut event = userdata::get_acc_event(key);
    let is_star_event = STAR_EVENT_IDS.contains(&event_id);
    println!("is_star_event: {}, {}", is_star_event, event_id);

    if event[event_id.to_string()].is_empty() {
        event[event_id.to_string()] = json::parse(&include_file!("src/router/userdata/new_user_event.json")).unwrap();
        if is_star_event {
            let mut ev = event[event_id.to_string()].clone();
            init_star_event(&mut ev);
            save_event_data(key, event_id, ev);
            event = userdata::get_acc_event(key);
        }
    }

    if is_star_event && event["star_last_reset"][event_id.to_string()].as_u64().unwrap_or(0) <= global::timestamp_since_midnight() {
        event["star_last_reset"][event_id.to_string()] = (global::timestamp_since_midnight() + (24 * 60 * 60)).into();
        event[event_id.to_string()]["star_event"]["star_event_bonus_daily_count"] = 0.into();
    }
    event[event_id.to_string()].clone()
}

fn save_event_data(key: &str, event_id: u32, data: JsonValue) {
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

    let body = &encryption::decrypt_packet(&body).unwrap();
    let body: EventGet = serde_json::from_str(body).unwrap();

    let event = get_event_data(&key, body.master_event_id);

    Some(event)
}

pub fn star_event(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);

    let body = &encryption::decrypt_packet(&body).unwrap();
    let body: StarEvent = serde_json::from_str(body).unwrap();

    let event = get_event_data(&key, body.master_event_id);

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

    let mut event = get_event_data(&key, body.master_event_id);

    event["star_event"]["music_change_count"] = (event["star_event"]["music_change_count"].as_i32().unwrap() + 1).into();

    switch_music(&mut event, body.position as i32);

    save_event_data(&key, body.master_event_id, event.clone());

    Some(event["star_event"].clone())
}

pub fn set_member(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);

    let body = &encryption::decrypt_packet(&body).unwrap();
    let body: EventSetMember = serde_json::from_str(body).unwrap();

    let mut event = get_event_data(&key, body.master_event_id);

    event["member_ranking"] = object!{
        master_character_id: body.master_character_id,
        rank: 0,
        point: 0
    };

    save_event_data(&key, body.master_event_id, event.clone());

    Some(object!{
        event_member: event["member_ranking"].clone()
    })
}

pub fn ranking(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{
        ranking_detail_list: []
    })
}

const POINTS_PER_LEVEL: i64 = 55;

fn get_star_rank(points: i64) -> i64 {
    ((points - (points % POINTS_PER_LEVEL)) / POINTS_PER_LEVEL) + 1
}

pub fn event_live(req: HttpRequest, body: String, skipped: bool) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body_temp = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let event_id = if skipped {
        body_temp["master_event_id"].as_u32().unwrap()
    } else {
        crate::router::live::get_end_live_event_id(&key, &body_temp)?
    };

    let mut resp = crate::router::live::live_end(&req, &body, skipped);
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut event = get_event_data(&key, event_id);

    let live_id = databases::LIVE_LIST[body["master_live_id"].to_string()]["masterMusicId"].as_i64().unwrap();
    let raw_score = body["live_score"]["score"].as_u64().unwrap_or(resp["high_score"].as_u64().unwrap());

    let bonus_event = event["star_event"]["star_event_bonus_daily_count"].as_u64().unwrap();
    let bonus_play_times = event["star_event"]["star_event_play_times_bonus_count"].as_u64().unwrap();
    let score = raw_score + (raw_score * bonus_event) + (raw_score * bonus_play_times);

    let mut all_clear = 1;
    let mut cleared = false;
    for data in event["star_event"]["star_music_list"].members_mut() {
        if data["master_music_id"] == live_id && score >= data["goal_score"].as_u64().unwrap() {
            data["is_cleared"] = 1.into();
            cleared = true;
        }
        if data["is_cleared"] == 0 {
            all_clear = 0;
        }
    }

    if cleared {
        event["star_event"]["star_event_bonus_daily_count"] = (event["star_event"]["star_event_bonus_daily_count"].as_u32().unwrap() + 1).into();
        event["star_event"]["star_event_bonus_count"] = (event["star_event"]["star_event_bonus_count"].as_u32().unwrap() + 1).into();
        event["star_event"]["star_event_play_times_bonus_count"] = (event["star_event"]["star_event_play_times_bonus_count"].as_u32().unwrap() + 1).into();


        event["point_ranking"]["point"] = (event["point_ranking"]["point"].as_i64().unwrap_or(0) + 31).into();
        event["star_event"]["star_level"] = get_star_rank(event["point_ranking"]["point"].as_i64().unwrap()).into();
    }

    resp["star_event_bonus_list"] = object!{
        "star_event_bonus": bonus_event,
        "star_event_bonus_score": bonus_event * raw_score,
        "star_play_times_bonus": bonus_play_times,
        "star_play_times_bonus_score": bonus_play_times * raw_score,
        "card_bonus": 0,
        "card_bonus_score": 0
    };

    resp["event_point_list"] = array![];
    resp["event_ranking_data"] = object! {
        "event_point_rank": event["point_ranking"]["point"].clone(),
        "next_reward_rank_point": 0,
        "event_score_rank": 0,
        "next_reward_rank_score": 0,
        "next_reward_rank_level": 0
    };


    resp["is_star_all_clear"] = all_clear.into();
    resp["star_level"] = event["star_event"]["star_level"].clone();
    resp["music_data"] = event["star_event"]["star_music_list"].clone();
    resp["total_score"] = score.into();
    resp["star_event"] = event["star_event"].clone();

    save_event_data(&key, event_id, event);

    println!("{}", resp);
    Some(resp)
}

pub fn event_end(req: HttpRequest, body: String) -> Option<JsonValue> {
    event_live(req, body, false)
}

pub fn event_skip(req: HttpRequest, body: String) -> Option<JsonValue> {
    event_live(req, body, true)
}

// Start request structs
// These start with CJsonSendParam in the source

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct StarEventChangeTargetMusic {
    master_event_id: u32,
    position: u32
}

#[derive(Serialize, Deserialize)]
struct EventGet {
    master_event_id: u32
}

#[derive(Serialize, Deserialize)]
struct EventSetMember {
    master_event_id: u32,
    master_character_id: u32
}

#[derive(Serialize, Deserialize)]
struct StarEvent {
    master_event_id: u32
}

/*
#[derive(Serialize, Deserialize)]
struct EventRankingGet {
    master_event_id: u32,
    ranking_type: i32,
    ranking_group_type: i32,
    user_id: u64,
    start_rank: u32,
    count: u64,
    group_id: u64
}
*/
