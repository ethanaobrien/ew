use rusqlite::{Connection, params, ToSql};
use std::sync::{Mutex, MutexGuard};
use lazy_static::lazy_static;
use json::{JsonValue, object};
use crate::router::global;
use rand::Rng;

lazy_static! {
    pub static ref ENGINE: Mutex<Option<Connection>> = Mutex::new(None);
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
fn create_store_v2(table: &str) {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                conn.execute(
                    table,
                    (),
                ).unwrap();
                return;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
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
    
    let mut new_user = json::parse(include_str!("new_user.json")).unwrap();
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
    
    let max = global::get_user_rank_data(user["user"]["exp"].as_i64().unwrap())["maxLp"].as_u64().unwrap();
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
    
    user["stamina"]["stamina"] = stamina.into();
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
