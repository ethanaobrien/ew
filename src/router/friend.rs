use json::{object, array, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{userdata, global};
use crate::encryption;

pub const FRIEND_LIMIT: usize = 40;

pub fn friend(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let friends = userdata::get_acc_friends(&key);
    
    let mut rv = array![];
    
    let rv_data = if body["status"].as_i32().unwrap() == 3 {
        friends["friend_user_id_list"].clone()
    } else if body["status"].as_i32().unwrap() == 2 {
        friends["pending_user_id_list"].clone()
    } else if body["status"].as_i32().unwrap() == 1 {
        friends["request_user_id_list"].clone()
    } else {
        array![]
    };
    
    for (_i, uid) in rv_data.members().enumerate() {
        rv.push(global::get_user(uid.as_i64().unwrap(), &friends, false)).unwrap();
    }
    
    Some(object!{
        "friend_list": rv
    })
}

pub fn ids(req: HttpRequest) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), "");
    let friends = userdata::get_acc_friends(&key);
    
    Some(friends)
}

pub fn recommend(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let friends = userdata::get_acc_friends(&key);
    
    let mut random = userdata::get_random_uids(20);
    let index = random.members().position(|r| *r.to_string() == user_id.to_string());
    if index.is_some() {
        random.array_remove(index.unwrap());
    }
    
    let mut rv = array![];
    for (_i, uid) in random.members().enumerate() {
        let user = global::get_user(uid.as_i64().unwrap(), &friends, false);
        if user["user"]["friend_request_disabled"] == "1" || user.is_empty() {
            continue;
        }
        rv.push(user).unwrap();
    }
    
    Some(object!{
        friend_list: rv
    })
}

pub fn search(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let friends = userdata::get_acc_friends(&key);
    
    let uid = body["user_id"].as_i64().unwrap();
    let user = global::get_user(uid, &friends, false);
    
    Some(user)
}

pub fn request(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);
    
    let uid = body["user_id"].as_i64().unwrap();
    if !userdata::friend_request_disabled(uid) {
        if !friends["request_user_id_list"].contains(uid) && friends["request_user_id_list"].len() < FRIEND_LIMIT { 
            friends["request_user_id_list"].push(uid).unwrap(); 
            userdata::save_acc_friends(&key, friends);
        }
        userdata::friend_request(uid, user_id);
    }
    
    Some(array![])
}

pub fn approve(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);
    
    let uid = body["user_id"].as_i64().unwrap();
    let index = friends["pending_user_id_list"].members().position(|r| *r.to_string() == uid.to_string());
    if index.is_some() {
        friends["pending_user_id_list"].array_remove(index.unwrap());
        if body["approve"] == "1" && ! friends["friend_user_id_list"].contains(uid) && friends["friend_user_id_list"].len() < FRIEND_LIMIT {
            friends["friend_user_id_list"].push(uid).unwrap();
        }
        
        userdata::friend_request_approve(uid, user_id, body["approve"] == "1", "request_user_id_list");
        userdata::save_acc_friends(&key, friends);
    }
    
    Some(array![])
}

pub fn cancel(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);
    
    let uid = body["user_id"].as_i64().unwrap();
    let index = friends["request_user_id_list"].members().position(|r| *r.to_string() == uid.to_string());
    if index.is_some() {
        friends["request_user_id_list"].array_remove(index.unwrap());
    }
    userdata::friend_request_approve(uid, user_id, false, "pending_user_id_list");
    userdata::save_acc_friends(&key, friends);
    
    Some(array![])
}

pub fn delete(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);
    
    let uid = body["user_id"].as_i64().unwrap();
    let index = friends["friend_user_id_list"].members().position(|r| *r.to_string() == uid.to_string());
    if index.is_some() {
        friends["friend_user_id_list"].array_remove(index.unwrap());
    }
    userdata::friend_remove(uid, user_id);
    userdata::save_acc_friends(&key, friends);
    
    Some(array![])
}
