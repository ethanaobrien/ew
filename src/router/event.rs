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
    //println!("is_star_event: {}, {}", is_star_event, event_id);

    // Broken event data.. Should no longer be possible.
    if is_star_event && event[event_id.to_string()]["star_event"]["star_music_list"].len() > 5 {
        event.remove(&event_id.to_string());
    }

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
    if !(1..=5).contains(&index) {
        return;
    }

    let mut i: i32 = -1;
    for (j, live) in event["star_event"]["star_music_list"].members().enumerate() {
        if live["position"] == index {
            i = j as i32;
            break;
        }
    }
    if i >= 0 {
        event["star_event"]["star_music_list"].array_remove(i as usize);
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

    let mut event = get_event_data(&key, body.master_event_id);

    let is_star_event = STAR_EVENT_IDS.contains(&body.master_event_id);

    if is_star_event {
        let user = userdata::get_acc(&key);
        let old = event["star_event"]["star_level"].as_i64().unwrap();
        event["star_event"]["star_level"] = get_star_rank(get_points(body.master_event_id, &user)).into();
        let leveled = old != event["star_event"]["star_level"].as_i64().unwrap();

        let mut all_clear = 1;
        for data in event["star_event"]["star_music_list"].members() {
            if data["is_cleared"] == 0 {
                all_clear = 0;
            }
        }
        if all_clear == 1 {
            event["star_event"]["star_music_list"] = array![];
            switch_music(&mut event, 1);
            switch_music(&mut event, 2);
            switch_music(&mut event, 3);
            switch_music(&mut event, 4);
            switch_music(&mut event, 5);
            save_event_data(&key, body.master_event_id, event.clone());
        }


        event["point_ranking"]["point"] = get_points(body.master_event_id, &user).into();
        event["point_ranking"]["rank"] = get_rank(body.master_event_id, user["user"]["id"].as_u64().unwrap()).into();

        if leveled {
            save_event_data(&key, body.master_event_id, event.clone());
            event["star_event"]["is_star_event_update"] = 1.into();
        } else {
            save_event_data(&key, body.master_event_id, event.clone());
        }
    }

    Some(event)
}

pub fn star_event(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let user = userdata::get_acc(&key);

    let body = &encryption::decrypt_packet(&body).unwrap();
    let body: StarEvent = serde_json::from_str(body).unwrap();

    let mut event = get_event_data(&key, body.master_event_id);

    let mut star_event = event["star_event"].clone();
    star_event["is_inherited_level_reward"] = 0.into();

    event["star_event"]["star_level"] = get_star_rank(get_points(body.master_event_id, &user)).into();
    star_event["is_star_level_up"] = 1.into();

    save_event_data(&key, body.master_event_id, event.clone());

    Some(object!{
        star_event: star_event,
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

fn get_rank(event: u32, user_id: u64) -> u32 {
    let scores = crate::router::event_ranking::get_raw_info(event);

    let mut i=1;
    for score in scores.members() {
        if score["user"] == user_id {
            return i;
        }
        i += 1;
    }
    0
}

pub async fn ranking(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = &encryption::decrypt_packet(&body).unwrap();
    let body: EventRankingGet = serde_json::from_str(body).unwrap();
    let scores = crate::router::event_ranking::get_scores_json().await[body.master_event_id.to_string()].clone();
    let mut rv = array![];
    let mut i=1;
    let start = if body.user_id == 0 { body.start_rank } else { get_rank(body.master_event_id, body.user_id) };
    for score in scores.members() {
        if i >= start && start + body.count >= i {
            rv.push(score.clone()).unwrap();
            i += 1;
        }
        if start + body.count >= i {
            break;
        }
    }

    Some(object!{
        ranking_detail_list: rv
    })
}

const POINTS_PER_LEVEL: i64 = 65;

fn get_star_rank(points: i64) -> i64 {
    ((points - (points % POINTS_PER_LEVEL)) / POINTS_PER_LEVEL) + 1
}

const LIMIT_COINS: i64 = 2000000000;

fn give_event_points(event_id: u32, amount: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for data in user["event_point_list"].members_mut() {
        if data["type"] == 1 {
            has = true;
            let new_amount = data["amount"].as_i64().unwrap() + amount;
            if new_amount > LIMIT_COINS {
                return true;
            }
            data["amount"] = new_amount.into();
            break;
        }
    }
    if !has {
        user["event_point_list"].push(object!{
            master_event_id: event_id,
            type: 1,
            amount: amount,
            reward_status: []
        }).unwrap();
    }
    false
}

fn get_points(event_id: u32, user: &JsonValue) -> i64 {
    for data in user["event_point_list"].members() {
        if data["type"] == 1 && data["master_event_id"] == event_id {
            return data["amount"].as_i64().unwrap()
        }
    }
    0
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
    let mut user = userdata::get_acc(&key);

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

        give_event_points(event_id, 31, &mut user);
        userdata::save_acc(&key, user.clone());
    }

    crate::router::event_ranking::live_completed(event_id, user["user"]["id"].as_i64().unwrap(), get_points(event_id, &user), event["star_event"]["star_level"].as_i64().unwrap());

    resp["star_event_bonus_list"] = object!{
        "star_event_bonus": bonus_event,
        "star_event_bonus_score": bonus_event * raw_score,
        "star_play_times_bonus": bonus_play_times,
        "star_play_times_bonus_score": bonus_play_times * raw_score,
        "card_bonus": 0,
        "card_bonus_score": 0
    };


    resp["event_point_list"] = user["event_point_list"].clone();
    resp["event_ranking_data"] = object! {
        "event_point_rank": event["point_ranking"]["point"].clone(),
        "next_reward_rank_point": 0,
        "event_score_rank": get_rank(event_id, user["user"]["id"].as_u64().unwrap()),
        "next_reward_rank_score": 0,
        "next_reward_rank_level": 0
    };

    resp["is_star_all_clear"] = all_clear.into();
    resp["star_level"] = event["star_event"]["star_level"].clone();
    resp["music_data"] = event["star_event"]["star_music_list"].clone();
    resp["total_score"] = score.into();
    resp["star_event"] = event["star_event"].clone();

    save_event_data(&key, event_id, event);

    //println!("{}", resp);
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

#[derive(Serialize, Deserialize)]
struct EventRankingGet {
    master_event_id: u32,
    ranking_type: i32,
    ranking_group_type: i32,
    user_id: u64,
    start_rank: u32,
    count: u32,
    group_id: u64
}
