use json;
use json::{array, object, JsonValue};
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;

pub fn deck(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    
    for (i, data) in user["deck_list"].members().enumerate() {
        if data["slot"].to_string() == body["slot"].to_string() {
            user["deck_list"][i] = body["main_card_ids"].clone();
            break;
        }
    }
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "deck": {
                "slot": body["slot"].clone(),
                "leader_role": 0,
                "main_card_ids": body["main_card_ids"].clone()
            },
            "clear_mission_ids": []
        }
    };
    global::send(resp)
}

pub fn user(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let user = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user
    };
    global::send(resp)
}

pub fn gift(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut user = userdata::get_acc_home(&key);
    let mut userr = userdata::get_acc(&key);
    
    let mut rewards = array![];
    let mut failed = array![];
    
    let mut to_remove = array![];
    for (_i, gift_id) in body["gift_ids"].members().enumerate() {
        for (j, data) in user["home"]["gift_list"].members_mut().enumerate() {
            if data["id"].to_string() != gift_id.to_string() {
                continue;
            }
            if data["reward_type"].to_string() == "1" {
                //gems
                userr["gem"]["free"] = (userr["gem"]["free"].as_i64().unwrap() + data["amount"].as_i64().unwrap()).into();
            //} else if data["reward_type"].to_string() == "3" {
                //not working
                /*
                //goes into user item_list
                let mut contains = false;
                for (_k, dataa) in userr["item_list"].members_mut().enumerate() {
                    if dataa["id"].to_string() != data["id"].to_string() {
                        continue;
                    }
                    contains = true;
                    dataa["amount"] = (dataa["amount"].as_i64().unwrap() + data["amount"].as_i64().unwrap()).into();
                    break;
                }
                if !contains {
                    let to_push = object!{
                        "id": data["id"].clone(),
                        "master_item_id": data["id"].clone(),//idk if this is correct
                        "amount": data["amount"].clone(),
                        "expire_date_time": null
                    };
                    userr["item_list"].push(to_push).unwrap();
                }*/
            } else {//idk
                println!("Redeeming reward not implimented for reward id {}", data["id"].to_string());
                failed.push(gift_id.clone()).unwrap();
                continue;
            }
            let to_push = object!{
                give_type: data["give_type"].clone(),
                type: data["reward_type"].clone(),
                value: data["value"].clone(),
                level: data["level"].clone(),
                amount: data["amount"].clone()
            };
            rewards.push(to_push).unwrap();
            to_remove.push(j).unwrap();
            
            break;
        }
    }
    for (_i, index) in to_remove.members().enumerate() {
        user["home"]["gift_list"].array_remove(index.as_usize().unwrap());
    }
    
    userdata::save_acc_home(&key, user.clone());
    userdata::save_acc(&key, userr.clone());
    let userr = userdata::get_acc(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "failed_gift_ids": failed,
            "updated_value_list": {
                "gem": userr["gem"].clone(),
                "item_list": userr["item_list"].clone()
            },
            "clear_mission_ids": user["clear_mission_ids"].clone(),
            "reward_list": rewards
        }
    };
    global::send(resp)
}

pub fn user_post(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut user = userdata::get_acc(&key);
    let user_2 = userdata::get_acc_home(&key);
    
    if !body["name"].is_null() {
        user["user"]["name"] = body["name"].clone();
    }
    if !body["comment"].is_null() {
        user["user"]["comment"] = body["comment"].clone();
    }
    if !body["favorite_master_card_id"].is_null() {
        user["user"]["favorite_master_card_id"] = body["favorite_master_card_id"].clone();
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
    
    userdata::save_acc(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "user": user["user"].clone(),
            "clear_mission_ids": user_2["clear_mission_ids"].clone()
        }
    };
    global::send(resp)
}

pub fn uid_to_code(uid: String) -> String {
    //just replace uid with numbers because im too lazy to have a real database and this is close enough anyways
    return uid
        .replace("1", "A")
        .replace("2", "G")
        .replace("3", "W")
        .replace("4", "Q")
        .replace("5", "Y")
        .replace("6", "6")
        .replace("7", "I")
        .replace("8", "P")
        .replace("9", "U")
        .replace("0", "M")
        + "7";
}
pub fn code_to_uid(code: String) -> String {
    //just replace uid with numbers because im too lazy to have a real database and this is close enough anyways
    return code
        .replace("7", "")
        .replace("A", "1")
        .replace("G", "2")
        .replace("W", "3")
        .replace("Q", "4")
        .replace("Y", "5")
        .replace("6", "6")
        .replace("I", "7")
        .replace("P", "8")
        .replace("U", "9")
        .replace("M", "0");
}

pub fn get_migration_code(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let code = uid_to_code(body["user_id"].to_string());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "migrationCode": code
        }
    };
    global::send(resp)
}

pub fn register_password(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let user = userdata::get_acc(&key);
    let code = uid_to_code(user["user"]["id"].to_string());
    
    userdata::save_acc_transfer(&code, &body["pass"].to_string());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": []
    };
    global::send(resp)
}

pub fn verify_migration_code(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let uid = code_to_uid(body["migrationCode"].to_string()).parse::<i64>().unwrap_or(0);
    
    let user = userdata::get_acc_transfer(uid, &body["migrationCode"].to_string(), &body["pass"].to_string());
    
    if user["success"].as_bool().unwrap() == false || uid == 0 {
        let resp = object!{
            "code": 2,
            "server_time": global::timestamp(),
            "message": ""
        };
        return global::send(resp);
    }
    
    let data_user = userdata::get_acc(&user["login_token"].to_string());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "user_id": uid,
            "uuid": user["login_token"].to_string(),
            "charge": data_user["gem"]["charge"].clone(),
            "free": data_user["gem"]["free"].clone()
        }
    };
    global::send(resp)
}
pub fn request_migration_code(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let uid = code_to_uid(body["migrationCode"].to_string()).parse::<i64>().unwrap_or(0);
    
    let user = userdata::get_acc_transfer(uid, &body["migrationCode"].to_string(), &body["pass"].to_string());
    
    if user["success"].as_bool().unwrap() != true || uid == 0 {
        let resp = object!{
            "code": 2,
            "server_time": global::timestamp(),
            "message": ""
        };
        return global::send(resp);
    }
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "twxuid": user["login_token"].to_string()
        }
    };
    global::send(resp)
}
pub fn migration(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let user = userdata::get_name_and_rank(body["user_id"].to_string().parse::<i64>().unwrap());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user
    };
    global::send(resp)
}
fn get_card(id: i64, user: &JsonValue) -> JsonValue {
    if id == 0 {
        return object!{};
    }
    
    for (_i, data) in user["card_list"].members().enumerate() {
        if data["master_card_id"].as_i64().unwrap_or(0) == id {
            return data.clone();
        }
    }
    return object!{};
}
fn get_cards(arr: JsonValue, user: &JsonValue) -> JsonValue {
    let mut rv = array![];
    for (_i, data) in arr.members().enumerate() {
        let to_push = get_card(data.as_i64().unwrap_or(0), user);
        if to_push.is_empty() {
            continue;
        }
        rv.push(to_push).unwrap();
    }
    return rv;
}

pub fn detail(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user_detail_list = array![];
    for (_i, data) in body["user_ids"].members().enumerate() {
        let uid = data.as_i64().unwrap();
        let user = userdata::get_acc_from_uid(uid);
        
        let mut to_push = object!{
            user: user["user"].clone(),
            live_data_summary: {
                clear_count_list: [0, 0, 0, 0],
                full_combo_list: [0, 0, 0, 0],
                all_perfect_list: [0, 0, 0, 0],
                high_score_rate: {
                    rate: 0,
                    detail: []
                }
            },
            main_deck_detail: {
                total_power: 0, //how to calculate?
                deck: user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1].clone(),
                card_list: get_cards(user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1]["main_card_ids"].clone(), &user)
            },
            favorite_card: get_card(user["user"]["favorite_master_card_id"].as_i64().unwrap_or(0), &user),
            guest_smile_card: get_card(user["user"]["guest_smile_master_card_id"].as_i64().unwrap_or(0), &user),
            guest_cool_card: get_card(user["user"]["guest_cool_master_card_id"].as_i64().unwrap_or(0), &user),
            guest_pure_card: get_card(user["user"]["guest_pure_master_card_id"].as_i64().unwrap_or(0), &user),
            master_title_ids: user["master_title_ids"].clone()
        };
        to_push["user"].remove("sif_user_id");
        to_push["user"].remove("ss_user_id");
        to_push["user"].remove("birthday");
        user_detail_list.push(to_push).unwrap();
    }
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            user_detail_list: user_detail_list
        }
    };
    global::send(resp)
}


pub fn initialize(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    let mut user = userdata::get_acc(&key);
    let mut user2 = userdata::get_acc_home(&key);
    let ur = user["card_list"][user["card_list"].len() - 1]["master_card_id"].clone();
    
    let id = ur.as_i32().unwrap(); //todo
    user["user"]["favorite_master_card_id"] = id.into();
    user["user"]["guest_smile_master_card_id"] = id.into();
    user["user"]["guest_cool_master_card_id"] = id.into();
    user["user"]["guest_pure_master_card_id"] = id.into();
    user2["home"]["preset_setting"][0]["illust_master_card_id"] = id.into();
    user["gem"]["free"] = (3000).into();
    user["gem"]["total"] = (3000).into();
    
    let id = body["master_character_id"].to_string();
    let userr = &id[id.len() - 2..].parse::<i32>().unwrap();
    
    let cardstoreward: JsonValue;
    let mut masterid = 3000000;
    if id.starts_with("1") {
        cardstoreward = array![10010001, 10020001, 10030001, 10040001, 10050001, 10060001, 10070001, 10080001, 10090001]; //muse
    } else if id.starts_with("2") {
        cardstoreward = array![20010001, 20020001, 20030001, 20040001, 20050001, 20060001, 20070001, 20080001, 20090001]; //aqours
        masterid += 9; //muse
    } else if id.starts_with("3") {
        cardstoreward = array![30010001, 30020001, 30030001, 30040001, 30050001, 30060001, 30070001, 30080001, 30090001, 30100001, 30110001]; //nijigasaki
        masterid += 9 + 9; //aqours
    } else if id.starts_with("4") {
        cardstoreward = array![40010001, 40020001, 40030001, 40040001, 40050001, 40060001, 40070001, 40080001, 40090001]; //liella
        masterid += 9 + 9 + 12; //nijigasaki
    } else {
        return global::error_resp();
    }
    masterid += userr;
    
    user["user"]["master_title_ids"][0] = masterid.into();
    
    // User is rewarded with all base cards in the team they chose. This makes up their new deck_list
    
    for (i, data) in cardstoreward.members().enumerate() {
        global::give_character(data.to_string(), &mut user);
        if i < 10 {
            user["deck_list"][0]["main_card_ids"][i] = data.clone();
        }
    }
    //todo - should the chosen character be in the team twice?
    user["deck_list"][0]["main_card_ids"][4] = ur;
    
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_home(&key, user2);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": user["user"].clone()
    };
    global::send(resp)
}
