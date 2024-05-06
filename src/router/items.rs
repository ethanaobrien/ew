use json::{array, object, JsonValue};
use lazy_static::lazy_static;
use rand::Rng;

use crate::router::global;

lazy_static! {
    static ref ITEM_INFO: JsonValue = {
        let mut info = object!{};
        let items = json::parse(include_str!("json/item.json")).unwrap();
        for (_i, data) in items.members().enumerate() {
            info[data["id"].to_string()] = data.clone();
        }
        info
    };
}

pub fn get_item_info(id: i64) -> JsonValue {
    ITEM_INFO[id.to_string()].clone()
}

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

pub fn give_gift(data: &JsonValue, user: &mut JsonValue) -> bool {
    if data.is_empty() {
        return false;
    }
    if data["reward_type"].to_string() == "1" {
        // basically primogems!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        return !give_primogems(data["amount"].as_i64().unwrap(), user);
    } else if data["reward_type"].to_string() == "2" {
        //character
        give_character(data["value"].to_string(), user);
        return true;
    } else if data["reward_type"].to_string() == "3" {
        return !give_item(data["value"].as_i64().unwrap(), data["amount"].as_i64().unwrap(), user);
    } else if data["reward_type"].to_string() == "4" {
        // basically moraa!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        return !give_points(data["value"].as_i64().unwrap(), data["amount"].as_i64().unwrap(), user);
    }
    println!("Redeeming reward not implimented for reward type {}", data["reward_type"].to_string());
    return false;
}
pub fn give_gift_basic(ty_pe: i32, id: i64, amount: i64, user: &mut JsonValue) -> bool {
    give_gift(&object!{
        reward_type: ty_pe,
        amount: amount,
        value: id
    }, user)
}
pub fn give_points(master_item_id: i64, amount: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for (_j, dataa) in user["point_list"].members_mut().enumerate() {
        if dataa["type"].as_i64().unwrap() == master_item_id {
            has = true;
            let new_amount = dataa["amount"].as_i64().unwrap() + amount;
            if new_amount > LIMIT_COINS {
                return true;
            }
            dataa["amount"] = new_amount.into();
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
    for (_j, dataa) in user["item_list"].members_mut().enumerate() {
        if dataa["master_item_id"].as_i64().unwrap() == master_item_id {
            if dataa["amount"].as_i64().unwrap() >= amount {
                dataa["amount"] = (dataa["amount"].as_i64().unwrap() - amount).into();
            } else {
                dataa["amount"] = (0).into();
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
    return to_push;
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
pub fn give_character(id: String, user: &mut JsonValue) -> bool {
    for (_i, data) in user["card_list"].members().enumerate() {
        if data["master_card_id"].to_string() == id || data["id"].to_string() == id {
            give_item(19100001, 50, user);
            return false;
        }
    }
    
    let to_push = object!{
        "id": id.parse::<i32>().unwrap(),
        "master_card_id": id.parse::<i32>().unwrap(),
        "exp": 0,
        "skill_exp": 0,
        "evolve": [],
        "created_date_time": global::timestamp()
    };
    user["card_list"].push(to_push.clone()).unwrap();
    true
}

pub fn get_user_rank_data(exp: i64) -> JsonValue {
    let ranks = json::parse(include_str!("userdata/user_rank.json")).unwrap();
    
    for (i, rank) in ranks.members().enumerate() {
        if exp < rank["exp"].as_i64().unwrap() {
            return ranks[i - 1].clone();
        }
    }
    return ranks[ranks.len() - 1].clone();
}

pub fn give_exp(amount: i32, user: &mut JsonValue) {
    let current_rank = get_user_rank_data(user["user"]["exp"].as_i64().unwrap());
    user["user"]["exp"] = (user["user"]["exp"].as_i32().unwrap() + amount).into();
    let new_rank = get_user_rank_data(user["user"]["exp"].as_i64().unwrap());
    if current_rank["rank"].to_string() != new_rank["rank"].to_string() {
        user["stamina"]["stamina"] = (user["stamina"]["stamina"].as_i64().unwrap() + new_rank["maxLp"].as_i64().unwrap()).into();
        user["stamina"]["last_updated_time"] = global::timestamp().into();
    }
}

pub fn update_mission_status(master_mission_id: i64, expire: u64, completed: bool, claimed: bool, advance: bool, missions: &mut JsonValue) -> Option<i64> {
    for (_i, mission) in missions.members_mut().enumerate() {
        if mission["master_mission_id"].as_i64().unwrap() == master_mission_id {
            mission["status"] = if claimed { 3 } else if completed { 2 } else { 1 }.into();
            if expire != 0 {
                mission["expire_date_time"] = expire.into();
            }
            
            if (mission["expire_date_time"].as_u64().unwrap() < global::timestamp() || expire != 0) && (mission["expire_date_time"].as_u64().unwrap() != 0 || expire != 0) {
                mission["progress"] = 0.into();
            }
            if advance {
                mission["progress"] = (mission["progress"].as_i32().unwrap() + 1).into();
            }
            
            if completed && !claimed {
                return Some(master_mission_id);
            }
            return None;
        }
    }
    None
}

pub fn update_mission_status_multi(master_mission_id: JsonValue, expire: u64, completed: bool, claimed: bool, advance: bool, missions: &mut JsonValue) -> JsonValue {
    let mut rv = array![];
    for (_i, mission) in master_mission_id.members().enumerate() {
        let val = update_mission_status(mission.as_i64().unwrap(), expire, completed, claimed, advance, missions);
        if !val.is_none() {
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

pub fn advance_variable_mission(min: i64, max: i64, missions: &mut JsonValue) -> JsonValue {
    let mut rv = array![];
    for i in min..=max {
        let mission_status = get_mission_status(i, missions);
        if mission_status.is_empty() {
            continue;
        }
        let mission_info = &crate::router::mission::MISSION_LIST[i.to_string()];
        if i == max && mission_info["conditionNumber"].as_i64().unwrap() <= mission_status["progress"].as_i64().unwrap() {
            break;
        }
        if mission_info["conditionNumber"].as_i64().unwrap() > mission_status["progress"].as_i64().unwrap() + 1 {
            if !update_mission_status(i, 0, false, false, true, missions).is_none() {
                rv.push(i).unwrap();
            }
        } else {
            if !update_mission_status(i, 0, true, false, true, missions).is_none() {
                rv.push(i).unwrap();
            }
        }
        break;
    }
    rv
}

pub fn completed_daily_mission(id: i64, missions: &mut JsonValue) -> JsonValue {
    let all_daily_missions = array![1224003, 1253003, 1273009, 1273010, 1273011, 1273012];
    
    let mission = get_mission_status(id, missions);
    if mission["expire_date_time"].as_u64().unwrap() >= global::timestamp() && mission["status"].as_i32().unwrap() > 1 {
        return array![];
    }
    let mut rv = array![];
    if id == 1253003 {
        rv = advance_variable_mission(1153001, 1153019, missions);
    }
    let mission = get_mission_status(1224003, missions);
    let next_reset = global::timestamp_since_midnight() + (24 * 60 * 60);
    if mission["expire_date_time"].as_u64().unwrap() < global::timestamp() {
        update_mission_status_multi(all_daily_missions, next_reset, false, false, false, missions);
    }
    
    if mission["progress"].as_i32().unwrap() == 4 {
        if !update_mission_status(1224003, 0, true, false, true, missions).is_none() {
            rv.push(1224003).unwrap();
        }
    } else {
        if !update_mission_status(1224003, 0, false, false, true, missions).is_none() {
            rv.push(1224003).unwrap();
        }
    }
    if !update_mission_status(id, next_reset, true, false, true, missions).is_none() {
        rv.push(id).unwrap();
    }
    rv
}
