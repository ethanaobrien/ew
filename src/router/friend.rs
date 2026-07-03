use jzon::{array, object};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, userdata};
use crate::encryption;
use crate::router::tools::guest;

pub const FRIEND_LIMIT: usize = 40;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/friend")
            .route("", web::post().to(friend))
            .route("/ids", web::get().to(ids))
            .route("/search", web::post().to(search))
            .route("/search/recommend", web::post().to(recommend))
            .route("/request", web::post().to(request))
            .route("/request/approve", web::post().to(approve))
            .route("/request/cancel", web::post().to(cancel))
            .route("/delete", web::post().to(delete))
    );
}

async fn friend(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
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

    for uid in rv_data.members() {
        let mut user = guest::get_user(uid.as_i64().unwrap(), &friends, guest::UserView::Card);
        user["user"]["last_login_time"] = global::set_time(user["user"]["last_login_time"].as_u64().unwrap_or(0), user_id, false).into();
        rv.push(user).unwrap();
    }

    global::api(&req, Some(object!{
        "friend_list": rv
    }))
}

async fn ids(req: HttpRequest) -> impl Responder {
    let key = global::get_login(req.headers(), "");
    let friends = userdata::get_acc_friends(&key);

    global::api(&req, Some(friends))
}

async fn recommend(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let friends = userdata::get_acc_friends(&key);

    let mut random = userdata::get_random_uids(20);
    let index = random.members().position(|r| *r.to_string() == user_id.to_string());
    if index.is_some() {
        random.array_remove(index.unwrap());
    }

    let mut rv = array![];
    for uid in random.members() {
        let mut user = guest::get_user(uid.as_i64().unwrap(), &friends, guest::UserView::Card);
        if user["user"]["friend_request_disabled"] == 1 || user.is_empty() {
            continue;
        }
        user["user"]["last_login_time"] = global::set_time(user["user"]["last_login_time"].as_u64().unwrap_or(0), user_id, false).into();
        rv.push(user).unwrap();
    }

    global::api(&req, Some(object!{
        friend_list: rv
    }))
}

async fn search(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let friends = userdata::get_acc_friends(&key);

    let uid = body["user_id"].as_i64().unwrap();
    let user = guest::get_user(uid, &friends, guest::UserView::Detail);

    global::api(&req, Some(if user.is_empty() {
        array![]
    } else {
        user
    }))
}

async fn request(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
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

    global::api(&req, Some(array![]))
}

async fn approve(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);

    let uid = body["user_id"].as_i64().unwrap();
    let index = friends["pending_user_id_list"].members().position(|r| *r.to_string() == uid.to_string());
    if index.is_some() {
        friends["pending_user_id_list"].array_remove(index.unwrap());
        if body["approve"] == 1 && ! friends["friend_user_id_list"].contains(uid) && friends["friend_user_id_list"].len() < FRIEND_LIMIT {
            friends["friend_user_id_list"].push(uid).unwrap();
        }

        userdata::friend_request_approve(uid, user_id, body["approve"] == 1, "request_user_id_list");
        userdata::save_acc_friends(&key, friends);
    }

    global::api(&req, Some(array![]))
}

async fn cancel(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);

    let uid = body["user_id"].as_i64().unwrap();
    let index = friends["request_user_id_list"].members().position(|r| *r.to_string() == uid.to_string());
    if index.is_some() {
        friends["request_user_id_list"].array_remove(index.unwrap());
    }
    userdata::friend_request_approve(uid, user_id, false, "pending_user_id_list");
    userdata::save_acc_friends(&key, friends);

    global::api(&req, Some(array![]))
}

async fn delete(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let user_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut friends = userdata::get_acc_friends(&key);

    let uid = body["user_id"].as_i64().unwrap();
    let index = friends["friend_user_id_list"].members().position(|r| *r.to_string() == uid.to_string());
    if index.is_some() {
        friends["friend_user_id_list"].array_remove(index.unwrap());
    }
    userdata::friend_remove(uid, user_id);
    userdata::save_acc_friends(&key, friends);

    global::api(&req, Some(array![]))
}
