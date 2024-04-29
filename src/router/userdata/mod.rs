use rusqlite::{Connection, params, ToSql};
use std::sync::{Mutex, MutexGuard};
use lazy_static::lazy_static;
use json::{JsonValue, array, object};
use crate::router::global;
use uuid::Uuid;
use rand::Rng;

lazy_static! {
    static ref ENGINE: Mutex<Option<Connection>> = Mutex::new(None);
    static ref NEW_USER: JsonValue = {
        json::parse(include_str!("new_user.json")).unwrap()
    };
}

fn init(engine: &mut MutexGuard<'_, Option<Connection>>) {
    let conn = Connection::open("userdata.db").unwrap();
    conn.execute("PRAGMA foreign_keys = ON;", ()).unwrap();

    engine.replace(conn);
}
fn lock_and_exec(command: &str, args: &[&dyn ToSql]) {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                conn.execute(command, args).unwrap();
                return;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

fn lock_and_select(command: &str, args: &[&dyn ToSql]) -> Result<String, rusqlite::Error> {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                let mut stmt = conn.prepare(command)?;
                return stmt.query_row(args, |row| {
                    match row.get::<usize, i64>(0) {
                        Ok(val) => Ok(val.to_string()),
                        Err(_) => row.get(0)
                    }
                });
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}
fn lock_and_select_all(command: &str, args: &[&dyn ToSql]) -> Result<JsonValue, rusqlite::Error> {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                let mut stmt = conn.prepare(command)?;
                let map = stmt.query_map(args, |row| {
                    match row.get::<usize, i64>(0) {
                        Ok(val) => Ok(val.to_string()),
                        Err(_) => row.get(0)
                    }
                })?;
                let mut rv = array![];
                for val in map {
                    let res = val?;
                    match res.clone().parse::<i64>() {
                        Ok(v) => rv.push(v).unwrap(),
                        Err(_) => rv.push(res).unwrap()
                    };
                }
                return Ok(rv);
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}
fn create_store_v2(table: &str) {
    lock_and_exec(table, params!());
}

fn create_token_store() {
    create_store_v2("CREATE TABLE IF NOT EXISTS tokens (
        user_id  BIGINT NOT NULL PRIMARY KEY,
        token    TEXT NOT NULL
    )");
}
fn create_uid_store() {
    create_store_v2("CREATE TABLE IF NOT EXISTS uids (
        user_id  BIGINT NOT NULL PRIMARY KEY
    )");
}
fn create_migration_store() {
    create_store_v2("CREATE TABLE IF NOT EXISTS migration (
        token     TEXT NOT NULL PRIMARY KEY,
        password  TEXT NOT NULL
    )");
}
fn create_users_store() {
    create_store_v2("CREATE TABLE IF NOT EXISTS users (
        user_id     BIGINT NOT NULL PRIMARY KEY,
        userdata    TEXT NOT NULL,
        userhome    TEXT NOT NULL,
        missions    TEXT NOT NULL,
        loginbonus  TEXT NOT NULL,
        sifcards    TEXT NOT NULL,
        friends     TEXT NOT NULL
    )");
}

fn acc_exists(uid: i64) -> bool {
    create_users_store();
    lock_and_select("SELECT user_id FROM users WHERE user_id=?1", params!(uid)).is_ok()
}
fn get_key(auth_key: &str) -> i64 {
    let uid = get_uid(&auth_key);
    let key = if uid == 0 {
        generate_uid()
    } else {
        uid
    };
    
    if !acc_exists(key) {
        create_acc(key, &auth_key);
    }
    
    key
}
fn uid_exists(uid: i64) -> bool {
    let data = lock_and_select("SELECT user_id FROM uids WHERE user_id=?1", params!(uid));
    data.is_ok()
}

fn generate_uid() -> i64 {
    create_uid_store();
    let mut rng = rand::thread_rng();
    let random_number = rng.gen_range(100_000_000_000_000..=999_999_999_999_999);
    //the chances of this...?
    if uid_exists(random_number) {
        return generate_uid();
    }
    lock_and_exec("INSERT INTO uids (user_id) VALUES (?1)", params!(random_number));
    
    random_number
}

fn create_acc(uid: i64, login: &str) {
    create_users_store();
    
    let mut new_user = NEW_USER.clone();
    new_user["user"]["id"] = uid.into();
    new_user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    lock_and_exec("INSERT INTO users (user_id, userdata, userhome, missions, loginbonus, sifcards, friends) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params!(
        uid,
        json::stringify(new_user),
        include_str!("new_user_home.json"),
        include_str!("chat_missions.json"),
        format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, global::timestamp()),
        "[]",
        r#"{"friend_user_id_list":[],"request_user_id_list":[],"pending_user_id_list":[]}"#
    ));
    
    create_token_store();
    lock_and_exec("DELETE FROM tokens WHERE token=?1", params!(login));
    lock_and_exec("INSERT INTO tokens (user_id, token) VALUES (?1, ?2)", params!(uid, login));
}

fn get_uid(token: &str) -> i64 {
    create_token_store();
    let data = lock_and_select("SELECT user_id FROM tokens WHERE token = ?1;", params!(token));
    if !data.is_ok() {
        return 0;
    }
    let data = data.unwrap();
    data.parse::<i64>().unwrap_or(0)
}

fn get_login_token(uid: i64) -> String {
    create_token_store();
    let data = lock_and_select("SELECT token FROM tokens WHERE user_id=?1", params!(uid));
    if !data.is_ok() {
        return String::new();
    }
    data.unwrap()
}

fn get_data(auth_key: &str, row: &str) -> JsonValue {
    let key = get_key(&auth_key);
    
    let result = lock_and_select(&format!("SELECT {} FROM users WHERE user_id=?1", row), params!(key));
    
    json::parse(&result.unwrap()).unwrap()
}

pub fn get_acc(auth_key: &str) -> JsonValue {
    let mut user = get_data(auth_key, "userdata");
    user["gem"]["total"] = (user["gem"]["charge"].as_i64().unwrap() + user["gem"]["free"].as_i64().unwrap()).into();
    if user["master_music_ids"].len() != 637 {
        user["master_music_ids"] = NEW_USER["master_music_ids"].clone();
    }
    
    global::lp_modification(&mut user, 0, false);
    return user;
}

pub fn get_acc_home(auth_key: &str) -> JsonValue {
    let mut user = get_data(auth_key, "userhome");
    user["home"]["pending_friend_count"] = get_acc_friends(auth_key)["pending_user_id_list"].len().into();
    
    return user;
}
pub fn get_acc_missions(auth_key: &str) -> JsonValue {
    get_data(auth_key, "missions")
}
pub fn get_acc_loginbonus(auth_key: &str) -> JsonValue {
    get_data(auth_key, "loginbonus")
}
pub fn get_acc_sif(auth_key: &str) -> JsonValue {
    get_data(auth_key, "sifcards")
}
pub fn get_acc_friends(auth_key: &str) -> JsonValue {
    get_data(auth_key, "friends")
}

pub fn save_data(auth_key: &str, row: &str, data: JsonValue) {
    let key = get_key(&auth_key);
    
    lock_and_exec(&format!("UPDATE users SET {}=?1 WHERE user_id=?2", row), params!(json::stringify(data), key));
}

pub fn save_acc(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "userdata", data);
}
pub fn save_acc_home(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "userhome", data);
}
pub fn save_acc_missions(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "missions", data);
}
pub fn save_acc_loginbonus(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "loginbonus", data);
}
pub fn save_acc_friends(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "friends", data);
}

pub fn get_acc_transfer(uid: i64, token: &str, password: &str) -> JsonValue {
    create_migration_store();
    let data = lock_and_select("SELECT password FROM migration WHERE token=?1", params!(token));
    if !data.is_ok() {
        return object!{success: false};
    }
    if data.unwrap().to_string() == password.to_string() {
        let login_token = get_login_token(uid);
        if login_token == String::new() {
            return object!{success: false};
        }
        return object!{success: true, login_token: login_token};
    }
    object!{success: false}
}

pub fn save_acc_transfer(token: &str, password: &str) {
    create_migration_store();
    lock_and_exec("DELETE FROM migration WHERE token=?1", params!(token));
    lock_and_exec("INSERT INTO migration (token, password) VALUES (?1, ?2)", params!(token, password));
}

pub fn get_name_and_rank(uid: i64) -> JsonValue {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return object!{
            user_name: "",
            user_rank: 1
        }
    }
    let uid = get_uid(&login_token);
    if uid == 0 || !acc_exists(uid) {
        return object!{
            user_name: "",
            user_rank: 1
        }
    }
    let result = lock_and_select("SELECT userdata FROM users WHERE user_id=?1", params!(uid));
    let data = json::parse(&result.unwrap()).unwrap();
    
    return object!{
        user_name: data["user"]["name"].clone(),
        user_rank: global::get_user_rank_data(data["user"]["exp"].as_i64().unwrap())["rank"].clone() //todo
    }
}

pub fn get_acc_from_uid(uid: i64) -> JsonValue {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return object!{
            error: true
        }
    }
    let uid = get_uid(&login_token);
    if uid == 0 || !acc_exists(uid) {
        return object!{"error": true}
    }
    let result = lock_and_select("SELECT userdata FROM users WHERE user_id=?1", params!(uid));
    json::parse(&result.unwrap()).unwrap()
}

pub fn friend_request(uid: i64, requestor: i64) {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return;
    }
    let uid = get_uid(&login_token);
    let friends = lock_and_select("SELECT friends FROM users WHERE user_id=?1", params!(uid));
    let mut friends = json::parse(&friends.unwrap()).unwrap();
    if !friends["pending_user_id_list"].contains(requestor) {
        friends["pending_user_id_list"].push(requestor).unwrap();
        lock_and_exec("UPDATE users SET friends=?1 WHERE user_id=?2", params!(json::stringify(friends), uid));
    }
}

pub fn friend_request_approve(uid: i64, requestor: i64, accepted: bool, key: &str) {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return;
    }
    let uid = get_uid(&login_token);
    let friends = lock_and_select("SELECT friends FROM users WHERE user_id=?1", params!(uid));
    let mut friends = json::parse(&friends.unwrap()).unwrap();
    let index = friends[key].members().into_iter().position(|r| *r.to_string() == requestor.to_string());
    if !index.is_none() {
        friends[key].array_remove(index.unwrap());
    }
    let index = friends["request_user_id_list"].members().into_iter().position(|r| *r.to_string() == requestor.to_string());
    if !index.is_none() {
        friends["request_user_id_list"].array_remove(index.unwrap());
    }
    if accepted && !friends["friend_user_id_list"].contains(requestor) {
        friends["friend_user_id_list"].push(requestor).unwrap();
    }
    lock_and_exec("UPDATE users SET friends=?1 WHERE user_id=?2", params!(json::stringify(friends), uid));
}

pub fn friend_request_disabled(uid: i64) -> bool {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return true;
    }
    let uid = get_uid(&login_token);
    let user = lock_and_select("SELECT userdata FROM users WHERE user_id=?1", params!(uid));
    let user = json::parse(&user.unwrap()).unwrap();
    user["user"]["friend_request_disabled"].to_string() == "1"
}

pub fn friend_remove(uid: i64, requestor: i64) {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return;
    }
    let uid = get_uid(&login_token);
    let friends = lock_and_select("SELECT friends FROM users WHERE user_id=?1", params!(uid));
    let mut friends = json::parse(&friends.unwrap()).unwrap();
    let index = friends["friend_user_id_list"].members().into_iter().position(|r| *r.to_string() == requestor.to_string());
    if !index.is_none() {
        friends["friend_user_id_list"].array_remove(index.unwrap());
    }
    lock_and_exec("UPDATE users SET friends=?1 WHERE user_id=?2", params!(json::stringify(friends), uid));
}

pub fn get_random_uids(count: i32) -> JsonValue {
    if count <= 0 {
        return array![];
    }
    lock_and_select_all(&format!("SELECT user_id FROM uids ORDER BY RANDOM() LIMIT {}", count), params!()).unwrap()
}

fn create_webui_store() {
    create_store_v2("CREATE TABLE IF NOT EXISTS webui (
        user_id      BIGINT NOT NULL PRIMARY KEY,
        token        TEXT NOT NULL,
        last_login   BIGINT NOT NULL
    )");
}

fn create_webui_token() -> String {
    let token = format!("{}", Uuid::new_v4());
    if lock_and_select("SELECT user_id FROM webui WHERE token=?1", params!(token)).is_ok() {
        return create_webui_token();
    }
    token
}

pub fn webui_login(uid: i64, password: &str) -> Result<String, String> {
    create_webui_store();
    create_migration_store();
    let pass = lock_and_select("SELECT password FROM migration WHERE token=?1", params!(crate::router::user::uid_to_code(uid.to_string()))).unwrap_or(String::new());
    if pass != password.to_string() || password == "" {
        if acc_exists(uid) {
            return Err(String::from("Migration token not set. Set token in game settings."));
        }
        return Err(String::from("User/password don't match"));
    }
    
    let new_token = create_webui_token();
    
    lock_and_exec("DELETE FROM webui WHERE user_id=?1", params!(uid));
    lock_and_exec("INSERT INTO webui (user_id, token, last_login) VALUES (?1, ?2, ?3)", params!(uid, new_token, global::timestamp()));
    Ok(new_token)
}

pub fn webui_import_user(user: JsonValue) -> Result<JsonValue, String> {
    let mut user = user;
    create_webui_store();
    create_migration_store();
    create_token_store();
    let uid = user["userdata"]["user"]["id"].as_i64().unwrap();
    if acc_exists(uid) {
        return Err(String::from("User already exists"));
    }
    if user["missions"].is_empty() {
        user["missions"] = json::parse(include_str!("chat_missions.json")).unwrap();
    }
    if user["sif_cards"].is_empty() {
        user["sif_cards"] = array![];
    }
    
    lock_and_exec("INSERT INTO users (user_id, userdata, userhome, missions, loginbonus, sifcards, friends) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params!(
        uid,
        json::stringify(user["userdata"].clone()),
        json::stringify(user["home"].clone()),
        json::stringify(user["missions"].clone()),
        format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, global::timestamp()),
        json::stringify(user["sif_cards"].clone()),
        r#"{"friend_user_id_list":[],"request_user_id_list":[],"pending_user_id_list":[]}"#
    ));
    
    let token;
    if !user["jp"].is_empty() {
        token = crate::router::gree::import_user(uid);
    } else {
        token = format!("{}", Uuid::new_v4());
    }
    
    lock_and_exec("INSERT INTO tokens (user_id, token) VALUES (?1, ?2)", params!(uid, token));
    let mig = crate::router::user::uid_to_code(uid.to_string());
    
    save_acc_transfer(&mig, &user["password"].to_string());
    
    Ok(object!{
        uid: uid,
        migration_token: mig
    })
}

fn webui_login_token(token: &str) -> Option<String> {
    let uid = lock_and_select("SELECT user_id FROM webui WHERE token=?1", params!(token)).unwrap_or(String::new());
    if uid == String::new() || token == "" {
        return None;
    }
    let uid = uid.parse::<i64>().unwrap_or(0);
    if uid == 0 {
        return None;
    }
    let last_login = lock_and_select("SELECT last_login FROM webui WHERE user_id=?1", params!(uid)).unwrap_or(String::new()).parse::<i64>().unwrap_or(0);
    let limit = 24 * 60 * 60; //1 day
    //Expired token
    if (global::timestamp() as i64) > last_login + limit {
        lock_and_exec("DELETE FROM webui WHERE user_id=?1", params!(uid));
        return None;
    }
    let login_token = lock_and_select("SELECT token FROM tokens WHERE user_id=?1", params!(uid)).unwrap_or(String::new());
    if login_token == String::new() {
        return None;
    }
    Some(login_token)
}

pub fn webui_get_user(token: &str) -> Option<JsonValue> {
    let login_token = webui_login_token(token)?;
    
    return Some(object!{
        userdata: get_acc(&login_token),
        loginbonus: get_acc_loginbonus(&login_token)
    });
}

pub fn webui_start_loginbonus(bonus_id: i64, token: &str) -> JsonValue {
    let login_token = webui_login_token(token);
    if login_token.is_none() {
        return object!{
            result: "ERR",
            message: "Failed to validate token"
        };
    }
    let login_token = login_token.unwrap();
    let mut bonuses = get_acc_loginbonus(&login_token);
    if !global::start_login_bonus(bonus_id, &mut bonuses) {
        return object!{
            result: "ERR",
            message: "Login bonus ID is either already going or does not exist"
        };
    }
    save_acc_loginbonus(&login_token, bonuses);
    
    return object!{
        result: "OK",
        id: bonus_id
    };
}

pub fn webui_logout(token: &str) {
    lock_and_exec("DELETE FROM webui WHERE token=?1", params!(token));
}
