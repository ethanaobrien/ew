use json::{array, object, JsonValue};
use rand::Rng;
use actix_web::{HttpRequest};
use crate::encryption;

use crate::router::{userdata, global, databases};

pub fn remove_gems(user: &mut JsonValue, amount: i64) {
    let mut amount = amount;
    let mut free = user["gem"]["free"].as_i64().unwrap();
    let mut paid = user["gem"]["charge"].as_i64().unwrap();
    
    free -= amount;
    if free < 0 {
        amount = -free;
        free = 0;
    }
    paid -= amount;
    if paid < 0 {
        paid = 0;
    }
    user["gem"]["free"] = free.into();
    user["gem"]["charge"] = paid.into();
    user["gem"]["total"] = (free + paid).into();
}

// true - limit reached
// false - all good
const GIFT_LIMIT: usize = 100000;
const LIMIT_ITEMS: i64 = 200000000;
const LIMIT_COINS: i64 = 2000000000;
const LIMIT_PRIMOGEMS: i64 = 1000000;

pub fn give_shop(master_item_id: i64, count: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for (_j, dataa) in user["shop_list"].members_mut().enumerate() {
        if dataa["master_shop_item_id"].as_i64().unwrap() == master_item_id {
            has = true;
            let new_amount = dataa["count"].as_i64().unwrap() + count;
            if new_amount > LIMIT_ITEMS {
                return true;
            }
            dataa["count"] = new_amount.into();
            break;
        }
    }
    if !has {
        user["shop_list"].push(object!{
            master_shop_item_id: master_item_id,
            count: count
        }).unwrap();
    }
    false
}

pub fn give_item(master_item_id: i64, amount: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for (_j, dataa) in user["item_list"].members_mut().enumerate() {
        if dataa["master_item_id"].as_i64().unwrap() == master_item_id {
            has = true;
            let new_amount = dataa["amount"].as_i64().unwrap() + amount;
            if new_amount > LIMIT_ITEMS {
                return true;
            }
            dataa["amount"] = new_amount.into();
            break;
        }
    }
    if !has {
        user["item_list"].push(object!{
            id: master_item_id,
            master_item_id: master_item_id,
            amount: amount,
            expire_date_time: null
        }).unwrap();
    }
    false
}

pub fn give_gift(data: &JsonValue, user: &mut JsonValue, missions: &mut JsonValue, clear_missions: &mut JsonValue) -> bool {
    if data.is_empty() {
        return false;
    }
    if data["reward_type"] == 1 {
        // basically primogems!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        return !give_primogems(data["amount"].as_i64().unwrap(), user);
    } else if data["reward_type"] == 2 {
        //character
        give_character(data["value"].as_i64().unwrap(), user, missions, clear_missions);
        return true;
    } else if data["reward_type"] == 3 {
        return !give_item(data["value"].as_i64().unwrap(), data["amount"].as_i64().unwrap(), user);
    } else if data["reward_type"] == 4 {
        // basically moraa!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        return !give_points(data["value"].as_i64().unwrap(), data["amount"].as_i64().unwrap(), user, missions, clear_missions);
    } else if data["reward_type"] == 8 {
        // title
        let title = data["value"].as_i64().unwrap();
        if !user["master_title_ids"].contains(title) {
            user["master_title_ids"].push(title).unwrap();
        }
        return true;
    }
    println!("Redeeming reward not implemented for reward type {}", data["reward_type"]);
    false
}
pub fn give_gift_basic(ty_pe: i32, id: i64, amount: i64, user: &mut JsonValue, missions: &mut JsonValue, clear_missions: &mut JsonValue) -> bool {
    give_gift(&object!{
        reward_type: ty_pe,
        amount: amount,
        value: id
    }, user, missions, clear_missions)
}
pub fn give_points(master_item_id: i64, amount: i64, user: &mut JsonValue, missions: &mut JsonValue, clear_missions: &mut JsonValue) -> bool {
    if master_item_id == 1 {
        let cleared = advance_variable_mission(1121001, 1121019, amount, missions);
        for (_i, data) in cleared.members().enumerate() {
            if !clear_missions.contains(data.as_i64().unwrap()) {
                clear_missions.push(data.clone()).unwrap();
            }
        }
    }
    let mut has = false;
    for (_j, data) in user["point_list"].members_mut().enumerate() {
        if data["type"].as_i64().unwrap() == master_item_id {
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
        user["point_list"].push(object!{
            type: master_item_id,
            amount: amount
        }).unwrap();
    }
    false
}

pub fn use_item(master_item_id: i64, amount: i64, user: &mut JsonValue) {
    for (_j, data) in user["item_list"].members_mut().enumerate() {
        if data["master_item_id"].as_i64().unwrap() == master_item_id {
            if data["amount"].as_i64().unwrap() >= amount {
                data["amount"] = (data["amount"].as_i64().unwrap() - amount).into();
            } else {
                data["amount"] = (0).into();
            }
            break;
        }
    }
}

pub fn give_primogems(amount: i64, user: &mut JsonValue) -> bool {
    let new_amount = user["gem"]["free"].as_i64().unwrap() + amount;
    if new_amount > LIMIT_PRIMOGEMS {
        return true;
    }
    
    user["gem"]["free"] = new_amount.into();
    false
}

pub fn gift_item(item: &JsonValue, reason: &str, user: &mut JsonValue) -> JsonValue {
    let to_push = object!{
        id: item["id"].clone(),
        reward_type: item["type"].clone(),
        is_receive: 0,
        reason_text: reason,
        value: item["value"].clone(),
        level: item["level"].clone(),
        amount: item["amount"].clone(),
        created_date_time: global::timestamp(),
        expire_date_time: global::timestamp() + (5 * (24 * 60 * 60)),
        received_date_time: 0
    };
    if user["home"]["gift_list"].len() >= GIFT_LIMIT {
        return to_push;
    }
    user["home"]["gift_list"].push(to_push.clone()).unwrap();
    to_push
}

fn random_number(lowest: usize, highest: usize) -> usize {
    if lowest == highest {
        return lowest;
    }
    assert!(lowest < highest);
    
    rand::thread_rng().gen_range(lowest..highest + 1)
}

pub fn gift_item_basic(id: i32, value: i64, ty_pe: i32, reason: &str, user: &mut JsonValue) -> JsonValue {
    gift_item(&object!{
        id: random_number(0, global::timestamp_msec() as usize),
        type: ty_pe,
        level: 0,
        amount: value,
        value: id
    }, reason, user)
}

pub fn lp_modification(user: &mut JsonValue, change_amount: u64, remove: bool) {
    let max = get_user_rank_data(user["user"]["exp"].as_i64().unwrap())["maxLp"].as_u64().unwrap();
    
    let speed = 285; //4 mins, 45 sec
    let since_last = global::timestamp() - user["stamina"]["last_updated_time"].as_u64().unwrap();
    
    let diff = since_last % speed;
    let restored = (since_last - diff) / speed;
    user["stamina"]["last_updated_time"] = (global::timestamp() - diff).into();
    
    let mut stamina = user["stamina"]["stamina"].as_u64().unwrap();
    if stamina < max {
        stamina += restored;
        if stamina > max {
            stamina = max;
        }
    }
    
    if remove {
        stamina -= change_amount;
    } else {
        stamina += change_amount;
    }
    
    user["stamina"]["stamina"] = stamina.into();
}

// true - added
// false - already has
pub fn give_character(id: i64, user: &mut JsonValue, missions: &mut JsonValue, clear_missions: &mut JsonValue) -> bool {
    for (_i, data) in user["card_list"].members().enumerate() {
        if data["master_card_id"] == id || data["id"] == id {
            give_item(19100001, 50, user);
            return false;
        }
    }
    let cleared = advance_variable_mission(1112001, 1112033, 1, missions);
    for (_i, data) in cleared.members().enumerate() {
        if !clear_missions.contains(data.as_i64().unwrap()) {
            clear_missions.push(data.clone()).unwrap();
        }
    }
    
    let to_push = object!{
        "id": id,
        "master_card_id": id,
        "exp": 0,
        "skill_exp": 0,
        "evolve": [],
        "created_date_time": global::timestamp()
    };
    user["card_list"].push(to_push.clone()).unwrap();
    true
}

pub fn get_user_rank_data(exp: i64) -> JsonValue {
    for (i, rank) in databases::RANKS.members().enumerate() {
        if exp < rank["exp"].as_i64().unwrap() {
            return databases::RANKS[i - 1].clone();
        }
    }
    databases::RANKS[databases::RANKS.len() - 1].clone()
}

pub fn give_exp(amount: i32, user: &mut JsonValue, mission: &mut JsonValue, rv: &mut JsonValue) {
    let current_rank = get_user_rank_data(user["user"]["exp"].as_i64().unwrap());
    user["user"]["exp"] = (user["user"]["exp"].as_i32().unwrap() + amount).into();
    let new_rank = get_user_rank_data(user["user"]["exp"].as_i64().unwrap());
    if current_rank["rank"] != new_rank["rank"] {
        user["stamina"]["stamina"] = (user["stamina"]["stamina"].as_i64().unwrap() + new_rank["maxLp"].as_i64().unwrap()).into();
        user["stamina"]["last_updated_time"] = global::timestamp().into();
        
        let status = get_mission_status(get_variable_mission_num(1101001, 1101030, mission), mission);
        if status.is_empty() {
            return;
        }
        let to_advance = new_rank["rank"].as_i64().unwrap() - status["progress"].as_i64().unwrap();
        let rvv = advance_variable_mission(1101001, 1101030, to_advance, mission);
        for (_i, id) in rvv.members().enumerate() {
            rv.push(id.as_i64().unwrap()).unwrap();
        }
    }
}

pub fn update_mission_status(master_mission_id: i64, expire: u64, completed: bool, claimed: bool, advance: i64, missions: &mut JsonValue) -> Option<i64> {
    for (_i, mission) in missions.members_mut().enumerate() {
        if mission["master_mission_id"].as_i64().unwrap() == master_mission_id {
            mission["status"] = if claimed { 3 } else if completed { 2 } else { 1 }.into();
            if expire != 0 {
                mission["expire_date_time"] = expire.into();
            }
            
            if (mission["expire_date_time"].as_u64().unwrap() < global::timestamp() || expire != 0) && (mission["expire_date_time"].as_u64().unwrap() != 0 || expire != 0) {
                mission["progress"] = 0.into();
            }
            if advance > 0 {
                mission["progress"] = (mission["progress"].as_i64().unwrap() + advance).into();
            }
            
            if completed && !claimed {
                return Some(master_mission_id);
            }
            return None;
        }
    }
    None
}

pub fn update_mission_status_multi(master_mission_id: JsonValue, expire: u64, completed: bool, claimed: bool, advance: i64, missions: &mut JsonValue) -> JsonValue {
    let mut rv = array![];
    for (_i, mission) in master_mission_id.members().enumerate() {
        let val = update_mission_status(mission.as_i64().unwrap(), expire, completed, claimed, advance, missions);
        if val.is_some() {
            rv.push(val.unwrap()).unwrap();
        }
    }
    rv
}

pub fn get_mission_status(id: i64, missions: &JsonValue) -> JsonValue {
    for (_i, mission) in missions.members().enumerate() {
        if mission["master_mission_id"].as_i64().unwrap() == id {
            return mission.clone();
        }
    }
    JsonValue::Null
}

pub fn change_mission_id(old: i64, new: i64, missions: &mut JsonValue) {
    for (_i, mission) in missions.members_mut().enumerate() {
        if mission["master_mission_id"].as_i64().unwrap() == old {
            mission["master_mission_id"] = new.into();
            return;
        }
    }
}

pub fn get_variable_mission_num(min: i64, max: i64, missions: &JsonValue) -> i64 {
    for i in min..=max {
        let mission_status = get_mission_status(i, missions);
        if mission_status.is_empty() {
            continue;
        }
        return i;
    }
    0
}

pub fn advance_variable_mission(min: i64, max: i64, count: i64, missions: &mut JsonValue) -> JsonValue {
    let mut rv = array![];
    for i in min..=max {
        let mission_status = get_mission_status(i, missions);
        if mission_status.is_empty() {
            continue;
        }
        let mission_info = &databases::MISSION_LIST[i.to_string()];
        if i == max && mission_info["conditionNumber"].as_i64().unwrap() <= mission_status["progress"].as_i64().unwrap() {
            break;
        }
        if mission_info["conditionNumber"].as_i64().unwrap() > mission_status["progress"].as_i64().unwrap() + count {
            if update_mission_status(i, 0, false, false, count, missions).is_some() {
                rv.push(i).unwrap();
            }
        } else if update_mission_status(i, 0, true, false, count, missions).is_some() {
            rv.push(i).unwrap();
        }
        break;
    }
    rv
}

pub fn advance_mission(id: i64, count: i64, max: i64, missions: &mut JsonValue) -> Option<i64> {
    let mission = get_mission_status(id, missions);
    
    if mission["status"].as_i32().unwrap() > 1 {
        return None;
    }
    let mut new = mission["progress"].as_i64().unwrap() + count;
    if new > max {
        new = max;
    }
    let completed = new == max;
    let advanced = new - mission["progress"].as_i64().unwrap();
    if update_mission_status(id, 0, completed, false, advanced, missions).is_some() {
        return Some(id);
    }
    None
}

pub fn completed_daily_mission(id: i64, missions: &mut JsonValue) -> JsonValue {
    let all_daily_missions = array![1224003, 1253003, 1273009, 1273010, 1273011, 1273012];
    
    let mission = get_mission_status(id, missions);
    if mission["expire_date_time"].as_u64().unwrap() >= global::timestamp() && mission["status"].as_i32().unwrap() > 1 {
        return array![];
    }
    let mut rv = array![];
    if id == 1253003 {
        rv = advance_variable_mission(1153001, 1153019, 1, missions);
    }
    let mission = get_mission_status(1224003, missions);
    let next_reset = global::timestamp_since_midnight() + (24 * 60 * 60);
    if mission["expire_date_time"].as_u64().unwrap() < global::timestamp() {
        update_mission_status_multi(all_daily_missions, next_reset, false, false, 0, missions);
    }
    
    if mission["progress"].as_i32().unwrap() == 4 {
        if update_mission_status(1224003, 0, true, false, 1, missions).is_some() {
            rv.push(1224003).unwrap();
        }
    } else if update_mission_status(1224003, 0, false, false, 1, missions).is_some() {
        rv.push(1224003).unwrap();
    }
    if update_mission_status(id, next_reset, true, false, 1, missions).is_some() {
        rv.push(id).unwrap();
    }
    rv
}

pub fn use_item_req(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    let item = &databases::ITEM_INFO[body["id"].to_string()];
    let amount = body["amount"].as_i64().unwrap();
    
    if item["effectType"].as_i32().unwrap() == 1 {
        lp_modification(&mut user, item["effectValue"].as_u64().unwrap() * (amount as u64), false);
    } else {
        println!("Use item not implemented for effect type {}", item["effectType"]);
    }
    use_item(body["id"].as_i64().unwrap(), amount, &mut user);
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        item_list: user["item_list"].clone(),
        stamina: user["stamina"].clone()
    })
}
