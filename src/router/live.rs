use json::{object, array, JsonValue};
use actix_web::{HttpResponse, HttpRequest};
use rand::Rng;
use lazy_static::lazy_static;

use crate::router::global;
use crate::encryption;
use crate::router::clear_rate::live_completed;
use crate::router::userdata;

pub fn retire(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    live_completed(body["master_live_id"].as_i64().unwrap(), body["level"].as_i32().unwrap(), true, 0, 0);
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

fn random_number(lowest: usize, highest: usize) -> usize {
    if lowest == highest {
        return lowest;
    }
    assert!(lowest < highest);
    
    rand::thread_rng().gen_range(lowest..highest + 1)
}

pub fn guest(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let friends = userdata::get_acc_friends(&key);
    let user = userdata::get_acc(&key);
    
    let mut guest_list = array![];
    if user["tutorial_step"].as_i32().unwrap() != 130 {
        guest_list.push(object!{
            "user": {
                "name": "A VERY Nice Guest",
                "comment": "Enjoy your first live show!",
                "exp": 900,
                "main_deck_slot": 1,
                "favorite_master_card_id": 10010013,
                "favorite_card_evolve": 0,
                "guest_smile_master_card_id": 10010013,
                "guest_cool_master_card_id": 10010013,
                "guest_pure_master_card_id": 10010013,
                "friend_request_disabled": 1,
                "master_title_ids": [3000001,0],
                "profile_settings": [1,2,3,4,5,6,7],
                "last_login_time": 1708699449
            },
            "favorite_card": {
                "id": 0,
                "master_card_id": 10010013,
                "exp": 1025,
                "skill_exp": 0,
                "evolve": []
            },
            "guest_smile_card": {
                "id": 0,
                "master_card_id": 10010013,
                "exp": 1025,
                "skill_exp": 0,
                "evolve": []
            },
            "guest_cool_card": {
                "id": 0,
                "master_card_id": 10010013,
                "exp": 1025,
                "skill_exp": 0,
                "evolve": []
            },
            "guest_pure_card": {
                "id": 0,
                "master_card_id": 10010013,
                "exp": 1025,
                "skill_exp": 0,
                "evolve": []
            },
            "status":0
        }).unwrap();
    } else {
        if friends["friend_user_id_list"].len() != 0 {
            guest_list.push(global::get_user(friends["friend_user_id_list"][random_number(0, friends["friend_user_id_list"].len() - 1)].as_i64().unwrap(), &friends, false)).unwrap();
        }
        let expected: usize = 5;
        if guest_list.len() < expected {
            let mut random = userdata::get_random_uids((expected-guest_list.len()) as i32);
            let index = random.members().into_iter().position(|r| *r.to_string() == user_id.to_string());
            if !index.is_none() {
                random.array_remove(index.unwrap());
            }
            
            for (_i, uid) in random.members().enumerate() {
                let guest = global::get_user(uid.as_i64().unwrap(), &friends, false);
                if guest["user"]["friend_request_disabled"].to_string() == "1" || guest.is_empty() {
                    continue;
                }
                guest_list.push(guest).unwrap();
            }
        }
        if guest_list.len() == 0 {
            guest_list.push(object!{
                "user": {
                    "name": "A sad Guest",
                    "comment": "Cant believe you're the only person on this server!",
                    "exp": 900,
                    "main_deck_slot": 1,
                    "favorite_master_card_id": 10010013,
                    "favorite_card_evolve": 0,
                    "guest_smile_master_card_id": 10010013,
                    "guest_cool_master_card_id": 10010013,
                    "guest_pure_master_card_id": 10010013,
                    "friend_request_disabled": 1,
                    "master_title_ids": [3000001,0],
                    "profile_settings": [1,2,3,4,5,6,7],
                    "last_login_time": 1708699449
                },
                "favorite_card": {
                    "id": 0,
                    "master_card_id": 10010013,
                    "exp": 1025,
                    "skill_exp": 0,
                    "evolve": []
                },
                "guest_smile_card": {
                    "id": 0,
                    "master_card_id": 10010013,
                    "exp": 1025,
                    "skill_exp": 0,
                    "evolve": []
                },
                "guest_cool_card": {
                    "id": 0,
                    "master_card_id": 10010013,
                    "exp": 1025,
                    "skill_exp": 0,
                    "evolve": []
                },
                "guest_pure_card": {
                    "id": 0,
                    "master_card_id": 10010013,
                    "exp": 1025,
                    "skill_exp": 0,
                    "evolve": []
                },
                "status":0
            }).unwrap();
        }
    }
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "guest_list": guest_list
        }
    };
    global::send(resp)
}

pub fn mission(_req: HttpRequest, _body: String) -> HttpResponse {
    //todo
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "score_ranking": "",
            "combo_ranking": "",
            "clear_count_ranking": ""
        }
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

pub fn continuee(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut user = userdata::get_acc(&key);
    
    global::remove_gems(&mut user, 100);
    
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "gem": user["gem"].clone()
    };
    global::send(resp)
}

pub fn update_live_data(user: &mut JsonValue, data: &JsonValue, add: bool) -> JsonValue {
    if user["tutorial_step"].as_i32().unwrap() < 130 {
        return JsonValue::Null;
    }
    
    let mut rv = object!{
        "master_live_id": data["master_live_id"].as_i64().unwrap(),
        "level": data["level"].as_i64().unwrap(),
        "clear_count": 1,
        "high_score": data["live_score"]["score"].as_i64().unwrap(),
        "max_combo": data["live_score"]["max_combo"].as_i64().unwrap(),
        "auto_enable": 1, //whats this?
        "updated_time": global::timestamp()
    };
    
    let mut has = false;
    for (_i, current) in user["live_list"].members_mut().enumerate() {
        if current["master_live_id"].to_string() == rv["master_live_id"].to_string() && current["level"].to_string() == rv["level"].to_string() {
            has = true;
            if add {
                rv["clear_count"] = (current["clear_count"].as_i64().unwrap() + 1).into();
            }
            current["clear_count"] = rv["clear_count"].clone();
            
            if rv["high_score"].as_i64().unwrap() > current["high_score"].as_i64().unwrap() {
                current["high_score"] = rv["high_score"].clone();
            } else {
                rv["high_score"] = current["high_score"].clone();
            }
            
            if rv["max_combo"].as_i64().unwrap() > current["max_combo"].as_i64().unwrap() {
                current["max_combo"] = rv["max_combo"].clone();
            } else {
                rv["max_combo"] = current["max_combo"].clone();
            }
            current["updated_time"] = rv["updated_time"].clone();
            break;
        }
    }
    if !has {
        user["live_list"].push(rv.clone()).unwrap()
    }
    rv
}
pub fn update_live_mission_data(user: &mut JsonValue, data: &JsonValue) {
    if user["tutorial_step"].as_i32().unwrap() < 130 {
        return;
    }
    
    let rv = object!{
        "master_live_id": data["master_live_id"].as_i64().unwrap(),
        "clear_master_live_mission_ids": data["clear_master_live_mission_ids"].clone()
    };
    
    for (_i, current) in user["live_mission_list"].members_mut().enumerate() {
        if current["master_live_id"].to_string() == rv["master_live_id"].to_string() {
            for (_i, id) in data["clear_master_live_mission_ids"].members().enumerate() {
                if !current["clear_master_live_mission_ids"].contains(id.as_i32().unwrap()) {
                    current["clear_master_live_mission_ids"].push(id.as_i32().unwrap()).unwrap();
                }
            }
            return;
        }
    }
    user["live_mission_list"].push(rv.clone()).unwrap();
}

lazy_static! {
    static ref LIVE_LIST: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/live.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
    static ref MISSION_DATA: JsonValue = {
        json::parse(include_str!("json/live_mission.json")).unwrap()
    };
    static ref MISSION_COMBO_DATA: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/live_mission_combo.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["masterMusicId"].to_string()] = data.clone();
        }
        info
    };
    static ref MISSION_REWARD_DATA: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/live_mission_reward.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
}
fn get_live_info(id: i64) -> JsonValue {
    LIVE_LIST[id.to_string()].clone()
}
fn get_live_combo_info(id: i64) -> JsonValue {
    MISSION_COMBO_DATA[id.to_string()].clone()
}
fn get_live_mission_info(id: i64) -> JsonValue {
    MISSION_REWARD_DATA[id.to_string()].clone()
}

fn get_live_mission_completed_ids(user: &JsonValue, live_id: i64, score: i64, combo: i64, clear_count: i64, level: i64) -> Option<JsonValue> {
    let live_info = get_live_info(live_id);
    let mut out = array![];
    let combo_info = get_live_combo_info(live_info["masterMusicId"].as_i64()?);
    
    for (_i, data) in MISSION_DATA.members().enumerate() {
        match data["type"].as_i32()? {
            1 => {
                if live_info[&format!("score{}", data["value"].to_string())].as_i64()? <= score {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            2 => {
                if combo_info["valueList"][data["value"].to_string().parse::<usize>().ok()?].as_i64()? <= combo {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            3 => {
                if combo_info["valueList"][3].as_i64()? <= combo && data["level"].as_i64()? == level {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            4 => {
                if clear_count >= data["value"].to_string().parse::<i64>().ok()? {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            _ => {}
        }
    }
    let mut rv = array![];
    for (_i, current) in user["live_mission_list"].members().enumerate() {
        if current["master_live_id"].to_string() == live_id.to_string() {
            for (_i, id) in out.members().enumerate() {
                if !current["clear_master_live_mission_ids"].contains(id.as_i32().unwrap()) {
                    rv.push(id.as_i32().unwrap()).unwrap();
                }
            }
            return Some(rv);
        }
    }
    Some(out)
}

fn give_mission_rewards(user: &mut JsonValue, missions: &JsonValue, multiplier: i64) -> JsonValue {
    let mut rv = array![];
    for (_i, data) in MISSION_DATA.members().enumerate() {
        if !missions.contains(data["id"].as_i32().unwrap()) {
            continue;
        }
        let mut gift = get_live_mission_info(data["masterLiveMissionRewardId"].as_i64().unwrap());
        gift["reward_type"] = gift["type"].clone();
        gift["amount"] = (gift["amount"].as_i64().unwrap() * multiplier).into();
        global::give_gift(&gift, user);
    }
    if global::give_gift_basic(3, 16005001, 10 * multiplier, user) {
        rv.push(object!{"type":3,"value":16005001,"level":0,"amount":10}).unwrap();
    }
    if global::give_gift_basic(3, 17001001, 2 * multiplier, user) {
        rv.push(object!{"type":3,"value":17001001,"level":0,"amount":2}).unwrap();
    }
    rv
}

pub fn end(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user2 = userdata::get_acc_home(&key);
    let mut user = userdata::get_acc(&key);
    let live = update_live_data(&mut user, &body, true);
    
    if body["live_score"]["score"].as_i64().unwrap() > 0 {
        live_completed(body["master_live_id"].as_i64().unwrap(), body["level"].as_i32().unwrap(), false, body["live_score"]["score"].as_i64().unwrap(), user["user"]["id"].as_i64().unwrap());
    }
    
    let missions = get_live_mission_completed_ids(&user, body["master_live_id"].as_i64().unwrap(), body["live_score"]["score"].as_i64().unwrap(), body["live_score"]["max_combo"].as_i64().unwrap(), live["clear_count"].as_i64().unwrap_or(0), body["level"].as_i64().unwrap()).unwrap_or(array![]);
    
    update_live_mission_data(&mut user, &object!{
        master_live_id: body["master_live_id"].as_i64().unwrap(),
        clear_master_live_mission_ids: missions.clone()
    });
    
    let reward_list = give_mission_rewards(&mut user, &missions, 1);
    
    global::lp_modification(&mut user, body["use_lp"].as_u64().unwrap(), true);
    
    global::give_exp(body["use_lp"].as_i32().unwrap(), &mut user);
    
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "gem": user["gem"].clone(),
            "high_score": live["high_score"].clone(),
            "item_list": user["item_list"].clone(),
            "point_list": user["point_list"].clone(),
            "live": live,
            "clear_master_live_mission_ids": missions,
            "user": user["user"].clone(),
            "stamina": user["stamina"].clone(),
            "character_list": user["character_list"].clone(),
            "reward_list": reward_list,
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

pub fn skip(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user2 = userdata::get_acc_home(&key);
    let mut user = userdata::get_acc(&key);
    let live = update_live_data(&mut user, &object!{
        master_live_id: body["master_live_id"].clone(),
        level: 1,
        live_score: {
            score: 1,
            max_combo: 1
        }
    }, false);
    
    let missions = get_live_mission_completed_ids(&user, body["master_live_id"].as_i64().unwrap(), live["high_score"].as_i64().unwrap(), live["max_combo"].as_i64().unwrap(), live["clear_count"].as_i64().unwrap(), live["level"].as_i64().unwrap()).unwrap_or(array![]);
    
    update_live_mission_data(&mut user, &object!{
        master_live_id: body["master_live_id"].as_i64().unwrap(),
        clear_master_live_mission_ids: missions.clone()
    });
    
    let reward_list = give_mission_rewards(&mut user, &missions, body["live_boost"].as_i64().unwrap());
    
    global::lp_modification(&mut user, 10 * body["live_boost"].as_u64().unwrap(), true);
    
    global::give_exp(10 * body["live_boost"].as_i32().unwrap(), &mut user);
    
    global::use_item(21000001, body["live_boost"].as_i64().unwrap(), &mut user);
    
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "gem": user["gem"].clone(),
            "high_score": live["high_score"].clone(),
            "item_list": user["item_list"].clone(),
            "point_list": user["point_list"].clone(),
            "live": live,
            "clear_master_live_mission_ids": missions,
            "user": user["user"].clone(),
            "stamina": user["stamina"].clone(),
            "character_list": user["character_list"].clone(),
            "reward_list": reward_list,
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
