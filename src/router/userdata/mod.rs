use rusqlite::{Connection, params};
use std::sync::{Mutex, MutexGuard};
use lazy_static::lazy_static;
use json::{JsonValue, array, object};
use crate::router::global;
use math::round;

lazy_static! {
    pub static ref ENGINE: Mutex<Option<Connection>> = Mutex::new(None);
}

fn init(engine: &mut MutexGuard<'_, Option<Connection>>) {
    let conn = Connection::open("userdata.db").unwrap();
    conn.execute("PRAGMA foreign_keys = ON;", ()).unwrap();

    engine.replace(conn);
}
fn create_token_store(conn: &Connection) {
    match conn.prepare("SELECT jsondata FROM tokens") {
        Ok(_) => {}
        Err(_) => {
            conn.execute(
                "CREATE TABLE tokens (
                    jsondata  TEXT NOT NULL
                )",
                (),
            ).unwrap();
            init_data(conn, "tokens", object!{});
        }
    }
}
fn create_uid_store(conn: &Connection) {
    match conn.prepare("SELECT jsondata FROM uids") {
        Ok(_) => {}
        Err(_) => {
            conn.execute(
                "CREATE TABLE uids (
                    jsondata  TEXT NOT NULL
                )",
                (),
            ).unwrap();
            init_data(conn, "uids", array![]);
        }
    }
}

fn create_migration_store(conn: &Connection) {
    match conn.prepare("SELECT jsondata FROM migrationdata") {
        Ok(_) => {}
        Err(_) => {
            conn.execute(
                "CREATE TABLE migrationdata (
                    jsondata  TEXT NOT NULL
                )",
                (),
            ).unwrap();
            init_data(conn, "migrationdata", object!{});
        }
    }
}
fn acc_exists(conn: &Connection, key: i64) -> bool {
    conn.prepare(&format!("SELECT jsondata FROM _{}_", key)).is_ok()
}
fn store_data(conn: &Connection, key: &str, value: JsonValue) {
    conn.execute(
        &format!("UPDATE {} SET jsondata=?1", key),
        params!(json::stringify(value))
    ).unwrap();
}
fn init_data(conn: &Connection, key: &str, value: JsonValue) {
    conn.execute(
        &format!("INSERT INTO {} (jsondata) VALUES (?1)", key),
        params!(json::stringify(value))
    ).unwrap();
}

use rand::Rng;
fn get_uids(conn: &Connection) -> JsonValue {
    let mut stmt = conn.prepare("SELECT jsondata FROM uids").unwrap();
    let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
    json::parse(&result.unwrap()).unwrap()
}
fn get_tokens(conn: &Connection) -> JsonValue {
    let mut stmt = conn.prepare("SELECT jsondata FROM tokens").unwrap();
    let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
    json::parse(&result.unwrap()).unwrap()
}

fn generate_uid(conn: &Connection) -> i64 {
    create_uid_store(conn);
    let mut rng = rand::thread_rng();
    let random_number = rng.gen_range(100_000_000_000_000..=999_999_999_999_999);
    let mut existing_ids = get_uids(conn);
    //the chances of this...?
    if existing_ids.contains(random_number) {
        return generate_uid(conn);
    }
    existing_ids.push(random_number).unwrap();
    store_data(conn, "uids", existing_ids);
    
    random_number
}

fn create_acc(conn: &Connection, uid: i64, login: &str) {
    let key = &uid.to_string();
    conn.execute(
        &format!("CREATE TABLE _{}_ (
            jsondata  TEXT NOT NULL
        )", key),
        (),
    ).unwrap();
    let mut data = object!{
        userdata: json::parse(include_str!("new_user.json")).unwrap(),
        home: json::parse(include_str!("new_user_home.json")).unwrap()
    };
    data["userdata"]["user"]["id"] = uid.into();
    data["userdata"]["stamina"]["last_updated_time"] = global::timestamp().into();
    
    init_data(conn, &format!("_{}_", key), data);
    
    create_token_store(conn);
    let mut tokens = get_tokens(conn);
    tokens[login] = uid.into();
    store_data(conn, "tokens", tokens);
}

fn get_uid(conn: &Connection, uid: &str) -> i64 {
    create_token_store(conn);
    let tokens = get_tokens(conn);
    if tokens[uid].is_null() {
        return 0;
    }
    return tokens[uid].as_i64().unwrap();
}

fn get_login_token(conn: &Connection, uid: i64) -> String {
    create_token_store(conn);
    let tokens = get_tokens(conn);
    for (_i, data) in tokens.entries().enumerate() {
        if uid == data.1.as_i64().unwrap() {
            return data.0.to_string();
        }
    }
    String::new()
}

fn get_data(a6573cbe: &str) -> JsonValue {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                let uid = get_uid(conn, &a6573cbe);
                
                let key: i64;
                if uid == 0 {
                    key = generate_uid(conn);
                    create_acc(conn, key, &a6573cbe);
                } else {
                    key = uid;
                }
                
                if !acc_exists(conn, key) {
                    create_acc(conn, key, &a6573cbe);
                }
                let mut stmt = conn.prepare(&format!("SELECT jsondata FROM _{}_", key)).unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let rv = json::parse(&result.unwrap()).unwrap();
                
                return rv
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub fn get_acc(a6573cbe: &str) -> JsonValue {
    let mut user = get_data(a6573cbe)["userdata"].clone();
    let max = 100; //todo
    let speed = 300; //5 mins
    let since_last = global::timestamp() - user["stamina"]["last_updated_time"].as_u64().unwrap();
    
    let restored = round::floor((since_last / speed) as f64, 0) as u64;
    let time_diff = since_last - (restored * speed);
    user["stamina"]["last_updated_time"] = (global::timestamp() - time_diff).into();
    let mut stamina = user["stamina"]["stamina"].as_u64().unwrap() + restored;
    if stamina > max {
        stamina = max;
    }
    
    user["stamina"]["stamina"] = stamina.into();
    return user;
}

pub fn get_acc_home(a6573cbe: &str) -> JsonValue {
    return get_data(a6573cbe)["home"].clone();
}

pub fn save_data(a6573cbe: &str, data: JsonValue, id: &str) {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                let uid = get_uid(conn, &a6573cbe);
                
                let key: i64;
                if uid == 0 {
                    key = generate_uid(conn);
                    create_acc(conn, key, &a6573cbe);
                } else {
                    key = uid;
                }
                
                if !acc_exists(conn, key) {
                    create_acc(conn, key, &a6573cbe);
                }
                let mut stmt = conn.prepare(&format!("SELECT jsondata FROM _{}_", key)).unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let mut rv = json::parse(&result.unwrap()).unwrap();
                
                rv[id] = data;
                store_data(conn, &format!("_{}_", key), rv);
                break;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub fn save_acc(a6573cbe: &str, data: JsonValue) {
    save_data(a6573cbe, data, "userdata");
}
pub fn save_acc_home(a6573cbe: &str, data: JsonValue) {
    save_data(a6573cbe, data, "home");
}

pub fn get_acc_transfer(uid: i64, token: &str, password: &str) -> JsonValue {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                create_migration_store(conn);
                
                let mut stmt = conn.prepare("SELECT jsondata FROM migrationdata").unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let data = json::parse(&result.unwrap()).unwrap();
                
                if data[token].is_empty() {
                    return object!{success: false};
                }
                if data[token].to_string() == password.to_string() {
                    let login_token = get_login_token(conn, uid);
                    if login_token == String::new() {
                        return object!{success: false};
                    }
                    return object!{success: true, login_token: login_token};
                }
                
                return object!{success: false};
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub fn save_acc_transfer(token: &str, password: &str) {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                create_migration_store(conn);
                
                let mut stmt = conn.prepare("SELECT jsondata FROM migrationdata").unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let mut data = json::parse(&result.unwrap()).unwrap();
                
                data[token] = password.into();
                
                store_data(conn, "migrationdata", data);
                break;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub fn get_name_and_rank(uid: i64) -> JsonValue {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                create_migration_store(conn);
                let login_token = get_login_token(conn, uid);
                if login_token == String::new() {
                    return object!{
                        user_name: "",
                        user_rank: 1
                    }
                }
                let uid = get_uid(conn, &login_token);
                if uid == 0 || !acc_exists(conn, uid) {
                    return object!{
                        user_name: "",
                        user_rank: 1
                    }
                }
                let mut stmt = conn.prepare(&format!("SELECT jsondata FROM _{}_", uid)).unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let data = json::parse(&result.unwrap()).unwrap();
                
                return object!{
                    user_name: data["userdata"]["user"]["name"].clone(),
                    user_rank: 1 //todo
                }
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub fn get_acc_from_uid(uid: i64) -> JsonValue {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                create_migration_store(conn);
                let login_token = get_login_token(conn, uid);
                if login_token == String::new() {
                    return object!{
                        user_name: "",
                        user_rank: 1
                    }
                }
                let uid = get_uid(conn, &login_token);
                if uid == 0 || !acc_exists(conn, uid) {
                    return object!{"error": true}
                }
                let mut stmt = conn.prepare(&format!("SELECT jsondata FROM _{}_", uid)).unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let data = json::parse(&result.unwrap()).unwrap();
                
                return data["userdata"].clone();
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}
