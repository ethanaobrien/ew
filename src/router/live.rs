use json::{object, array, JsonValue};
use actix_web::{HttpRequest};
use rand::Rng;
use lazy_static::lazy_static;

use crate::router::{global, userdata, items, databases};
use crate::encryption;
use crate::router::clear_rate::live_completed;

pub fn retire(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    if body["live_score"]["play_time"].as_i64().unwrap_or(0) > 5 {
        live_completed(body["master_live_id"].as_i64().unwrap(), body["level"].as_i32().unwrap(), true, 0, 0);
    }
    Some(object!{
        "stamina": {},
        "item_list": [],
        "event_point_list": []
    })
}

pub fn reward(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{
        "ensured_list": [],
        "random_list": []
    })
}

fn random_number(lowest: usize, highest: usize) -> usize {
    if lowest == highest {
        return lowest;
    }
    assert!(lowest < highest);
    
    rand::thread_rng().gen_range(lowest..highest + 1)
}

pub fn guest(req: HttpRequest, body: String) -> Option<JsonValue> {
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
        if !friends["friend_user_id_list"].is_empty() {
            guest_list.push(global::get_user(friends["friend_user_id_list"][random_number(0, friends["friend_user_id_list"].len() - 1)].as_i64().unwrap(), &friends, false)).unwrap();
        }
        let expected: usize = 5;
        if guest_list.len() < expected {
            let mut random = userdata::get_random_uids((expected-guest_list.len()) as i32);
            let index = random.members().position(|r| *r.to_string() == user_id.to_string());
            if index.is_some() {
                random.array_remove(index.unwrap());
            }
            
            for uid in random.members() {
                let guest = global::get_user(uid.as_i64().unwrap(), &friends, false);
                if guest["user"]["friend_request_disabled"] == 1 || guest.is_empty() {
                    continue;
                }
                guest_list.push(guest).unwrap();
            }
        }
        if guest_list.is_empty() {
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
    
    Some(object!{
        "guest_list": guest_list
    })
}

pub fn mission(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    //todo
    Some(object!{
        "score_ranking": "",
        "combo_ranking": "",
        "clear_count_ranking": ""
    })
}

pub fn start(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(array![])
}

pub fn event_start(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(array![])
}

pub fn continuee(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let mut user = userdata::get_acc(&key);
    
    items::remove_gems(&mut user, 100);
    
    userdata::save_acc(&key, user.clone());
    
    Some(user["gem"].clone())
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
    for current in user["live_list"].members_mut() {
        if current["master_live_id"] == rv["master_live_id"] && (current["level"] == rv["level"] || data["level"].as_i32().unwrap() == 0) {
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
            rv["level"] = current["level"].clone();
            if data["level"].as_i32().unwrap() != 0 {
                break;
            }
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
    
    for current in user["live_mission_list"].members_mut() {
        if current["master_live_id"] == rv["master_live_id"] {
            for id in data["clear_master_live_mission_ids"].members() {
                if !current["clear_master_live_mission_ids"].contains(id.as_i32().unwrap()) {
                    current["clear_master_live_mission_ids"].push(id.as_i32().unwrap()).unwrap();
                }
            }
            return;
        }
    }
    user["live_mission_list"].push(rv.clone()).unwrap();
}

fn get_live_mission_completed_ids(user: &JsonValue, live_id: i64, score: i64, combo: i64, clear_count: i64, level: i64, full_combo: bool, all_perfect: bool) -> Option<JsonValue> {
    let live_info = &databases::LIVE_LIST[live_id.to_string()];
    let mut out = array![];
    let combo_info = &databases::MISSION_COMBO_DATA[live_info["masterMusicId"].to_string()];
    
    for data in databases::MISSION_DATA.members() {
        match data["type"].as_i32()? {
            1 => {
                if live_info[&format!("score{}", data["value"])].as_i64()? <= score {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            2 => {
                if combo_info["valueList"][data["value"].to_string().parse::<usize>().ok()?].as_i64()? <= combo {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            3 => {
                if full_combo && combo_info["valueList"][3].as_i64()? <= combo && data["level"].as_i64()? == level {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            4 => {
                if clear_count >= data["value"].to_string().parse::<i64>().ok()? {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            5 => {
                if full_combo && all_perfect && data["level"].as_i64()? == level {
                    out.push(data["id"].as_i32()?).ok()?;
                }
            },
            _ => {}
        }
    }
    let mut rv = array![];
    for current in user["live_mission_list"].members() {
        if current["master_live_id"] == live_id {
            for id in out.members() {
                if !current["clear_master_live_mission_ids"].contains(id.as_i32().unwrap()) {
                    rv.push(id.as_i32().unwrap()).unwrap();
                }
            }
            return Some(rv);
        }
    }
    Some(out)
}

fn give_mission_rewards(user: &mut JsonValue, missions: &JsonValue, user_missions: &mut JsonValue, cleared_missions: &mut JsonValue, multiplier: i64) -> JsonValue {
    let mut rv = array![];
    for data in databases::MISSION_DATA.members() {
        if !missions.contains(data["id"].as_i32().unwrap()) {
            continue;
        }
        if data["masterLiveMissionRewardId"].as_i64().unwrap() == 0 {
            continue;
        }
        let mut gift = databases::MISSION_REWARD_DATA[data["masterLiveMissionRewardId"].to_string()].clone();
        gift["reward_type"] = gift["type"].clone();
        gift["amount"] = (gift["amount"].as_i64().unwrap() * multiplier).into();
        items::give_gift(&gift, user, user_missions, cleared_missions);
    }
    if items::give_gift_basic(3, 16005001, 10 * multiplier, user, user_missions, cleared_missions) {
        rv.push(object!{"type":3,"value":16005001,"level":0,"amount":10}).unwrap();
    }
    if items::give_gift_basic(3, 17001001, 2 * multiplier, user, user_missions, cleared_missions) {
        rv.push(object!{"type":3,"value":17001001,"level":0,"amount":2}).unwrap();
    }
    rv
}

fn get_master_id(id: i64) -> i64 {
    let id = id.to_string();
    let mut masterid = 0;
    if id.starts_with('2') {
        masterid += 9;
    } else if id.starts_with('3') {
        masterid += 9 + 9;
    } else if id.starts_with('4') {
        masterid += 9 + 9 + 12;
    }
    masterid + id.char_indices().last().unwrap().1.to_string().parse::<i64>().unwrap()
}

const MAX_BOND: i64 = 500000;

lazy_static! {
    pub static ref BOND_WEIGHT: JsonValue = {
        array![1, 1, 2, 2, 5, 2, 2, 1, 1]
    };
}

fn get_live_character_list(lp_used: i32, deck_id: i32, user: &JsonValue, missions: &mut JsonValue, completed_missions: &mut JsonValue) -> JsonValue {
    let mut rv = array![];
    let mut has = array![];
    let mut has_i = array![];
    let characters_in_deck = user["deck_list"][(deck_id - 1) as usize]["main_card_ids"].clone();
    let mut i = 0;
    for data in user["card_list"].members() {
        if !characters_in_deck.contains(data["id"].as_i64().unwrap()) && !characters_in_deck.contains(data["master_card_id"].as_i64().unwrap())  {
            continue;
        }
        let character = databases::CARD_LIST[data["master_card_id"].to_string()]["masterCharacterId"].as_i64().unwrap();
        let mut mission_id = 1158000 + get_master_id(character);
        let mut full = false;
        let mut status = items::get_mission_status(mission_id, missions);
        let mut limit = 1500;
        
        if status.is_empty() {
            mission_id += 39;
            limit *= 10;
            status = items::get_mission_status(mission_id, missions);
            if status["status"].as_i32().unwrap_or(0) > 1 {
                full = true;
            }
        }
        
        let mut index = characters_in_deck.members().position(|r| *r == data["id"]);
        if index.is_none() {
            index = characters_in_deck.members().position(|r| *r == data["master_card_id"]);
        }
        let exp = BOND_WEIGHT[index.unwrap_or(10)].as_i32().unwrap_or(0) * (lp_used / 10);
        let additional_exp;
        if has.contains(character) {
            additional_exp = 0;
            let j = has.members().position(|r| r == character).unwrap_or(10);
            if j != 10 {
                let start = rv[has_i[j].as_usize().unwrap()]["before_exp"].as_i64().unwrap();
                let mut bond = start + exp as i64;
                if bond >= MAX_BOND { bond = MAX_BOND; }
                if bond > rv[has_i[j].as_usize().unwrap()]["exp"].as_i64().unwrap() {
                    let completed = bond >= limit;
                    let mission = items::update_mission_status(mission_id, 0, completed, false, bond - start, missions);
                    if mission.is_some() {
                        completed_missions.push(mission.unwrap()).unwrap();
                    }
                    rv[has_i[j].as_usize().unwrap()]["exp"] = bond.into();
                    has_i[j] = i.into();
                }
            }
        } else {
            has.push(character).unwrap();
            has_i.push(i).unwrap();
            additional_exp = exp;
        }
        
        let start = status["progress"].as_i64().unwrap_or(0);
        let mut bond = start + additional_exp as i64;
        if bond >= MAX_BOND {
            bond = MAX_BOND;
        }
        if !full && additional_exp > 0 {
            let completed = bond >= limit;
            let mission = items::update_mission_status(mission_id, 0, completed, false, bond - start, missions);
            if mission.is_some() {
                completed_missions.push(mission.unwrap()).unwrap();
            }
        }
        
        rv.push(object!{
            master_character_id: character,
            exp: bond,
            before_exp: start
        }).unwrap();
        i += 1;
    }
    rv
}

fn live_end(req: &HttpRequest, body: &str, skipped: bool) -> JsonValue {
    let key = global::get_login(req.headers(), body);
    let body = json::parse(&encryption::decrypt_packet(body).unwrap()).unwrap();
    let user2 = userdata::get_acc_home(&key);
    let mut user = userdata::get_acc(&key);
    let mut user_missions = userdata::get_acc_missions(&key);

    let live = if skipped {
        items::use_item(&object!{
            value: 21000001,
            amount: 1,
            consumeType: 4
        }, body["live_boost"].as_i64().unwrap(), &mut user);
        update_live_data(&mut user, &object!{
            master_live_id: body["master_live_id"].clone(),
            level: 0,
            live_score: {
                score: 1,
                max_combo: 1
            }
        }, false)
    } else {
        update_live_data(&mut user, &body, true)
    };
    
    //1273009, 1273010, 1273011, 1273012
    let mut cleared_missions = items::advance_variable_mission(1105001, 1105017, 1, &mut user_missions);
    if body["master_live_id"].to_string().len() > 1 {
        let id = body["master_live_id"].to_string().split("").collect::<Vec<_>>()[2].parse::<i64>().unwrap_or(0);
        if (1..=4).contains(&id) {
            let to_push = items::completed_daily_mission(1273009 + id - 1, &mut user_missions);
            for data in to_push.members() {
                cleared_missions.push(data.as_i32().unwrap()).unwrap();
            }
        }
    }
    
    let missions;
    if skipped {
        live_completed(body["master_live_id"].as_i64().unwrap(), live["level"].as_i32().unwrap(), false, live["high_score"].as_i64().unwrap(), user["user"]["id"].as_i64().unwrap());
        missions = get_live_mission_completed_ids(&user, body["master_live_id"].as_i64().unwrap(), live["high_score"].as_i64().unwrap(), live["max_combo"].as_i64().unwrap(), live["clear_count"].as_i64().unwrap(), live["level"].as_i64().unwrap(), false, false).unwrap_or(array![]);
    } else {
        live_completed(body["master_live_id"].as_i64().unwrap(), body["level"].as_i32().unwrap(), false, body["live_score"]["score"].as_i64().unwrap(), user["user"]["id"].as_i64().unwrap());
        let is_full_combo = (body["live_score"]["good"].as_i32().unwrap_or(1) + body["live_score"]["bad"].as_i32().unwrap_or(1) + body["live_score"]["miss"].as_i32().unwrap_or(1)) == 0;
        let is_perfect = (body["live_score"]["great"].as_i32().unwrap_or(1) + body["live_score"]["good"].as_i32().unwrap_or(1) + body["live_score"]["bad"].as_i32().unwrap_or(1) + body["live_score"]["miss"].as_i32().unwrap_or(1)) == 0;
        missions = get_live_mission_completed_ids(&user, body["master_live_id"].as_i64().unwrap(), body["live_score"]["score"].as_i64().unwrap(), body["live_score"]["max_combo"].as_i64().unwrap(), live["clear_count"].as_i64().unwrap_or(0), body["level"].as_i64().unwrap(), is_full_combo, is_perfect).unwrap_or(array![]);
    
        if is_full_combo {
            if items::advance_mission(1176001, 1, 1, &mut user_missions).is_some() {
                cleared_missions.push(1176001).unwrap();
            }
            if items::advance_mission(1176002, 1, 100, &mut user_missions).is_some() {
                cleared_missions.push(1176002).unwrap();
            }
            if items::advance_mission(1176003, 1, 200, &mut user_missions).is_some() {
                cleared_missions.push(1176003).unwrap();
            }
            if items::advance_mission(1176004, 1, 300, &mut user_missions).is_some() {
                cleared_missions.push(1176004).unwrap();
            }
            if items::advance_mission(1176005, 1, 400, &mut user_missions).is_some() {
                cleared_missions.push(1176005).unwrap();
            }
            if items::advance_mission(1176006, 1, 500, &mut user_missions).is_some() {
                cleared_missions.push(1176006).unwrap();
            }
        }
        if is_perfect && items::advance_mission(1177001, 1, 1, &mut user_missions).is_some() {
            cleared_missions.push(1177001).unwrap();
        }
        if is_perfect && body["level"].as_i32().unwrap() == 4 && items::advance_mission(1177002, 1, 1, &mut user_missions).is_some() {
            cleared_missions.push(1177002).unwrap();
        }
    }
    
    update_live_mission_data(&mut user, &object!{
        master_live_id: body["master_live_id"].as_i64().unwrap(),
        clear_master_live_mission_ids: missions.clone()
    });
    
    let reward_list = give_mission_rewards(&mut user, &missions, &mut user_missions, &mut cleared_missions, body["live_boost"].as_i64().unwrap_or(1));
    
    let lp_used: i32 = body["use_lp"].as_i32().unwrap_or(10 * body["live_boost"].as_i32().unwrap_or(0));
    
    items::lp_modification(&mut user, lp_used as u64, true);
    
    items::give_exp(lp_used, &mut user, &mut user_missions, &mut cleared_missions);
    
    let characters = get_live_character_list(lp_used, body["deck_slot"].as_i32().unwrap_or(user["user"]["main_deck_slot"].as_i32().unwrap()), &user, &mut user_missions, &mut cleared_missions);
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_missions(&key, user_missions);
    
    object!{
        "gem": user["gem"].clone(),
        "high_score": live["high_score"].clone(),
        "item_list": user["item_list"].clone(),
        "point_list": user["point_list"].clone(),
        "live": live,
        "clear_master_live_mission_ids": missions,
        "user": user["user"].clone(),
        "stamina": user["stamina"].clone(),
        "character_list": characters,
        "reward_list": reward_list,
        "gift_list": user2["home"]["gift_list"].clone(),
        "clear_mission_ids": cleared_missions,
        "event_point_reward_list": [],
        "ranking_change": [],
        "event_member": [],
        "event_ranking_data": []
    }
}

pub fn end(req: HttpRequest, body: String) -> Option<JsonValue> {
    Some(live_end(&req, &body, false))
}

pub fn skip(req: HttpRequest, body: String) -> Option<JsonValue> {
    Some(live_end(&req, &body, true))
}

pub fn event_end(req: HttpRequest, body: String) -> Option<JsonValue> {
    let mut resp = live_end(&req, &body, false);
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut event = userdata::get_acc_event(&key);
    
    let live_id = databases::LIVE_LIST[body["master_live_id"].to_string()]["masterMusicId"].as_i64().unwrap();
    
    let mut all_clear = 1;
    for data in event["event_data"]["star_event"]["star_music_list"].members_mut() {
        if data["master_music_id"].as_i64().unwrap() == live_id {
            data["is_cleared"] = 1.into();
        }
        if !data["is_cleared"].as_i32().unwrap() == 0 {
            all_clear = 0;
        }
    }
    
    resp["event_point_list"] = array![];
    resp["event_ranking_data"] = object!{
        "event_point_rank": event["event_data"]["point_ranking"]["point"].clone(),
        "next_reward_rank_point": 0,
        "event_score_rank": 0,
        "next_reward_rank_score": 0,
        "next_reward_rank_level": 0
    };
    resp["star_level"] = event["event_data"]["star_event"]["star_level"].clone();
    resp["music_data"] = event["event_data"]["star_event"]["star_music_list"].clone();
    resp["is_star_all_clear"] = all_clear.into();
    resp["star_event_bonus_list"] = object!{
        "star_event_bonus": 0,
        "star_event_bonus_score": 0,
        "star_play_times_bonus": 0,
        "star_play_times_bonus_score": 0,
        "card_bonus": 0,
        "card_bonus_score": 0
    };
    resp["total_score"] = body["live_score"]["score"].clone();
    resp["star_event"] = object!{
        "star_event_bonus_daily_count": event["event_data"]["point_ranking"]["star_event_bonus_daily_count"].clone(),
        "star_event_bonus_count": event["event_data"]["point_ranking"]["star_event_bonus_count"].clone(),
        "star_event_play_times_bonus_count": event["event_data"]["point_ranking"]["star_event_play_times_bonus_count"].clone()
    };
    
    userdata::save_acc_event(&key, event);
    
    Some(resp)
}
