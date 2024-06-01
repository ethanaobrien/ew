use json::{array, object, JsonValue};
use actix_web::{HttpRequest};

use crate::encryption;
use crate::router::{userdata, global, items};
use crate::include_file;

pub fn deck(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    for (i, data) in user["deck_list"].clone().members().enumerate() {
        if data["slot"].as_usize().unwrap_or(100) != i + 1 && i < 10 {
            user["deck_list"][i] = object!{
                "slot": i + 1,
                "leader_role": 0,
                "main_card_ids": [0, 0, 0, 0, 0, 0, 0, 0, 0]
            }
        }
    }
    
    for data in user["deck_list"].members_mut() {
        if data["slot"].as_i32().unwrap() == body["slot"].as_i32().unwrap() {
            data["main_card_ids"] = body["main_card_ids"].clone();
            break;
        }
    }
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        "deck": {
            "slot": body["slot"].clone(),
            "leader_role": 0,
            "main_card_ids": body["main_card_ids"].clone()
        },
        "clear_mission_ids": []
    })
}

pub fn user(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let mut user = userdata::get_acc(&key);
    
    user["lottery_list"] = array![];
    
    Some(user)
}

pub fn gift(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut user = userdata::get_acc_home(&key);
    let mut userr = userdata::get_acc(&key);
    let mut missions = userdata::get_acc_missions(&key);
    
    let mut cleared_missions = array![];
    let mut rewards = array![];
    let mut failed = array![];
    
    for gift_id in body["gift_ids"].members() {
        let mut to_remove = 0;
        for (j, data) in user["home"]["gift_list"].members_mut().enumerate() {
            if data["id"] != *gift_id {
                continue;
            }
            if !items::give_gift(data, &mut userr, &mut missions, &mut cleared_missions) {
                failed.push(gift_id.clone()).unwrap();
                break;
            }
            let to_push = object!{
                give_type: 2,
                type: data["reward_type"].clone(),
                value: data["value"].clone(),
                level: data["level"].clone(),
                amount: data["amount"].clone()
            };
            rewards.push(to_push).unwrap();
            to_remove = j + 1;
            break;
        }
        if to_remove != 0 {
            user["home"]["gift_list"].array_remove(to_remove - 1);
        }
    }
    
    userdata::save_acc_missions(&key, missions);
    userdata::save_acc_home(&key, user);
    userdata::save_acc(&key, userr.clone());
    let userr = userdata::get_acc(&key);

    Some(object!{
        "failed_gift_ids": failed,
        "updated_value_list": {
            "gem": userr["gem"].clone(),
            "item_list": userr["item_list"].clone(),
            "point_list": userr["point_list"].clone(),
            "card_list": userr["card_list"].clone()
        },
        "clear_mission_ids": cleared_missions,
        "reward_list": rewards
    })
}

pub fn user_post(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut user = userdata::get_acc(&key);
    
    if !body["name"].is_null() {
        user["user"]["name"] = body["name"].clone();
    }
    if !body["comment"].is_null() {
        user["user"]["comment"] = body["comment"].clone();
    }
    if !body["favorite_master_card_id"].is_null() {
        user["user"]["favorite_master_card_id"] = body["favorite_master_card_id"].clone();
        user["user"]["favorite_card_evolve"] = if global::get_card(body["favorite_master_card_id"].as_i64().unwrap(), &user)["evolve"].is_empty() { 0 } else { 1 }.into();
    }
    if !body["guest_smile_master_card_id"].is_null() {
        user["user"]["guest_smile_master_card_id"] = body["guest_smile_master_card_id"].clone();
    }
    if !body["guest_pure_master_card_id"].is_null() {
        user["user"]["guest_pure_master_card_id"] = body["guest_pure_master_card_id"].clone();
    }
    if !body["guest_cool_master_card_id"].is_null() {
        user["user"]["guest_cool_master_card_id"] = body["guest_cool_master_card_id"].clone();
    }
    if !body["friend_request_disabled"].is_null() {
        user["user"]["friend_request_disabled"] = body["friend_request_disabled"].clone();
    }
    if !body["profile_settings"].is_null() {
        user["user"]["profile_settings"] = body["profile_settings"].clone();
    }
    if !body["main_deck_slot"].is_null() {
        user["user"]["main_deck_slot"] = body["main_deck_slot"].clone();
    }
    if !body["master_title_ids"].is_null() {
        user["user"]["master_title_ids"][0] = body["master_title_ids"][0].clone();
    }
    if !body["birthday"].is_null() {
        user["user"]["birthday"][0] = body["birthday"].clone();
    }
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        "user": user["user"].clone(),
        "clear_mission_ids": []
    })
}

pub fn announcement(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    
    let mut user = userdata::get_acc_home(&key);
    
    user["home"]["new_announcement_flag"] = (0).into();
    
    userdata::save_acc_home(&key, user);
    
    Some(object!{
        new_announcement_flag: 0
    })
}

pub fn uid_to_code(uid: String) -> String {
    //just replace uid with numbers because im too lazy to have a real database and this is close enough anyways
    uid
        .replace('1', "A")
        .replace('2', "G")
        .replace('3', "W")
        .replace('4', "Q")
        .replace('5', "Y")
        .replace('6', "6")
        .replace('7', "I")
        .replace('8', "P")
        .replace('9', "U")
        .replace('0', "M")
        + "7"
}
pub fn code_to_uid(code: String) -> String {
    //just replace uid with numbers because im too lazy to have a real database and this is close enough anyways
    code
        .replace('7', "")
        .replace('A', "1")
        .replace('G', "2")
        .replace('W', "3")
        .replace('Q', "4")
        .replace('Y', "5")
        .replace('6', "6")
        .replace('I', "7")
        .replace('P', "8")
        .replace('U', "9")
        .replace('M', "0")
}

pub fn get_migration_code(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let code = uid_to_code(body["user_id"].to_string());
    
    Some(object!{
        "migrationCode": code
    })
}

pub fn register_password(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let user = userdata::get_acc(&key);
    let code = uid_to_code(user["user"]["id"].to_string());
    
    userdata::save_acc_transfer(&code, &body["pass"].to_string());
    
    Some(array![])
}

pub fn verify_migration_code(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let uid = code_to_uid(body["migrationCode"].to_string()).parse::<i64>().unwrap_or(0);
    
    let user = userdata::get_acc_transfer(uid, &body["migrationCode"].to_string(), &body["pass"].to_string());
    
    if !user["success"].as_bool().unwrap() || uid == 0 {
        return None;
    }
    
    let data_user = userdata::get_acc(&user["login_token"].to_string());
    
    Some(object!{
        "user_id": uid,
        "uuid": user["login_token"].to_string(),
        "charge": data_user["gem"]["charge"].clone(),
        "free": data_user["gem"]["free"].clone()
    })
}
pub fn request_migration_code(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let uid = code_to_uid(body["migrationCode"].to_string()).parse::<i64>().unwrap_or(0);
    
    let user = userdata::get_acc_transfer(uid, &body["migrationCode"].to_string(), &body["pass"].to_string());
    
    if !user["success"].as_bool().unwrap() || uid == 0 {
        return None;
    }
    
    Some(object!{
        "twxuid": user["login_token"].to_string()
    })
}
pub fn migration(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let user = userdata::get_name_and_rank(body["user_id"].to_string().parse::<i64>().unwrap());
    
    Some(user)
}

pub fn detail(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let friends = userdata::get_acc_friends(&key);
    
    let mut user_detail_list = array![];
    for data in body["user_ids"].members() {
        let uid = data.as_i64().unwrap();
        let user = global::get_user(uid, &friends, true);
        user_detail_list.push(user).unwrap();
    }
    Some(object!{
        user_detail_list: user_detail_list
    })
}

pub fn sif(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    let mut cards = userdata::get_acc_sif(&key);
    
    // prevent duplicate data in the database
    if user["user"]["sif_user_id"].as_i64().unwrap() == 111111111 {
        cards = json::parse(&include_file!("src/router/userdata/full_sif.json")).unwrap();
    }
    
    Some(object!{
        cards: cards
    })
}

pub fn sifas_migrate(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{
        "ss_migrate_status": 1,
        "user": null,
        "gift_list": null,
        "lock_remain_time": null
    })
}

pub fn sif_migrate(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let mut user = userdata::get_acc(&key);
    user["user"]["sif_user_id"] = 111111111.into();
    
    userdata::save_acc(&key, user.clone());
    
    Some(object!{
        "sif_migrate_status": 0,
        "user": user["user"].clone(),
        "master_title_ids": user["master_title_ids"].clone()
    })
    
    
    /*
    // Error response
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "sif_migrate_status": 38
        }
    };
    */
}

pub fn getregisteredplatformlist(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{
        "google": 0,
        "apple": 0,
        "twitter": 0
    })
}

pub fn initialize(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut user = userdata::get_acc(&key);
    let mut user2 = userdata::get_acc_home(&key);
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let id = body["master_character_id"].as_i64().unwrap();
    
    crate::router::chat::add_chat(id, 1, &mut chats);
    
    let id = id.to_string();
    
    let ur = user["card_list"][0]["master_card_id"].as_i64().unwrap();
    
    user["user"]["favorite_master_card_id"] = ur.into();
    user["user"]["guest_smile_master_card_id"] = ur.into();
    user["user"]["guest_cool_master_card_id"] = ur.into();
    user["user"]["guest_pure_master_card_id"] = ur.into();
    user2["home"]["preset_setting"][0]["illust_master_card_id"] = ur.into();
    user["gem"]["free"] = (3000).into();
    user["gem"]["total"] = (3000).into();
    
    let userr = &id[id.len() - 2..].parse::<i32>().unwrap();
    
    let cardstoreward: JsonValue;
    let mut masterid = 3000000;
    if id.starts_with('1') {
        cardstoreward = array![10010001, 10020001, 10030001, 10040001, 10050001, 10060001, 10070001, 10080001, 10090001]; //muse
    } else if id.starts_with('2') {
        cardstoreward = array![20010001, 20020001, 20030001, 20040001, 20050001, 20060001, 20070001, 20080001, 20090001]; //aqours
        masterid += 9; //muse
    } else if id.starts_with('3') {
        cardstoreward = array![30010001, 30020001, 30030001, 30040001, 30050001, 30060001, 30070001, 30080001, 30090001, 30100001, 30110001]; //nijigasaki
        masterid += 9 + 9; //aqours
    } else if id.starts_with('4') {
        cardstoreward = array![40010001, 40020001, 40030001, 40040001, 40050001, 40060001, 40070001, 40080001, 40090001]; //liella
        masterid += 9 + 9 + 12; //nijigasaki
    } else {
        return None;
    }
    masterid += userr;
    
    user["user"]["master_title_ids"][0] = masterid.into();
    
    // User is rewarded with all base cards in the team they chose. This makes up their new deck_list
    
    for (i, data) in cardstoreward.members().enumerate() {
        items::give_character(data.as_i64().unwrap(), &mut user, &mut missions, &mut array![]);
        if i < 10 {
            user["deck_list"][0]["main_card_ids"][i] = data.clone();
        }
    }
    //todo - should the chosen character be in the team twice?
    user["deck_list"][0]["main_card_ids"][4] = ur.into();
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_home(&key, user2);
    userdata::save_acc_missions(&key, missions);
    
    Some(user["user"].clone())
}
