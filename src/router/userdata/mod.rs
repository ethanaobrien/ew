use rusqlite::{Connection, params, ToSql};
use std::sync::{Mutex, MutexGuard};
use lazy_static::lazy_static;
use json::{JsonValue, object};
use crate::router::global;
use math::round;
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
fn lock_and_select(command: &str) -> Result<String, rusqlite::Error> {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                let mut stmt = conn.prepare(command)?;
                return stmt.query_row([], |row| row.get(0));
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}
fn create_store(test_cmd: &str, table: &str, init_cmd: &str, init_args: &[&dyn ToSql]) {
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                match conn.prepare(test_cmd) {
                    Ok(_) => {}
                    Err(_) => {
                        conn.execute(
                            table,
                            (),
                        ).unwrap();
                        conn.execute(
                            init_cmd,
                            init_args
                        ).unwrap();
                    }
                }
                return;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

fn create_token_store() {
    create_store("SELECT jsondata FROM tokens", "CREATE TABLE tokens (
        jsondata  TEXT NOT NULL
    )", "INSERT INTO tokens (jsondata) VALUES (?1)", params!("{}"));
}
fn create_uid_store() {
    create_store("SELECT jsondata FROM uids", "CREATE TABLE uids (
        jsondata  TEXT NOT NULL
    )", "INSERT INTO uids (jsondata) VALUES (?1)", params!("[]"));
}
fn create_migration_store() {
    create_store("SELECT jsondata FROM migrationdata", "CREATE TABLE migrationdata (
        jsondata  TEXT NOT NULL
    )", "INSERT INTO migrationdata (jsondata) VALUES (?1)", params!("{}"));
}

fn acc_exists(key: i64) -> bool {
    lock_and_select(&format!("SELECT userdata FROM _{}_", key)).is_ok()
}
fn store_data(key: &str, value: JsonValue) {
    lock_and_exec(&format!("UPDATE {} SET jsondata=?1", key), params!(json::stringify(value)));
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
fn get_uids() -> JsonValue {
    let data = lock_and_select("SELECT jsondata FROM uids");
    json::parse(&data.unwrap()).unwrap()
}
fn get_tokens() -> JsonValue {
    let data = lock_and_select("SELECT jsondata FROM tokens");
    json::parse(&data.unwrap()).unwrap()
}

fn generate_uid() -> i64 {
    create_uid_store();
    let mut rng = rand::thread_rng();
    let random_number = rng.gen_range(100_000_000_000_000..=999_999_999_999_999);
    let mut existing_ids = get_uids();
    //the chances of this...?
    if existing_ids.contains(random_number) {
        return generate_uid();
    }
    existing_ids.push(random_number).unwrap();
    store_data("uids", existing_ids);
    
    random_number
}

fn create_acc(uid: i64, login: &str) {
    let key = &uid.to_string();
    
    let mut new_user = json::parse(include_str!("new_user.json")).unwrap();
    new_user["user"]["id"] = uid.into();
    new_user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    create_store(&format!("SELECT userhome FROM _{}_", key), &format!("CREATE TABLE _{}_ (
            userdata    TEXT NOT NULL,
            userhome    TEXT NOT NULL,
            missions    TEXT NOT NULL,
            loginbonus  TEXT NOT NULL,
            sifcards    TEXT NOT NULL
        )", key),
        &format!("INSERT INTO _{}_ (userdata, userhome, missions, loginbonus, sifcards) VALUES (?1, ?2, ?3, ?4, ?5)", key),
        params!(
            json::stringify(new_user),
            include_str!("new_user_home.json"),
            include_str!("chat_missions.json"),
            format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, global::timestamp()),
            "[]"
        )
    );
    
    create_token_store();
    let mut tokens = get_tokens();
    tokens[login] = uid.into();
    store_data("tokens", tokens);
}

fn get_uid(uid: &str) -> i64 {
    create_token_store();
    let tokens = get_tokens();
    if tokens[uid].is_null() {
        return 0;
    }
    return tokens[uid].as_i64().unwrap();
}

fn get_login_token(uid: i64) -> String {
    create_token_store();
    let tokens = get_tokens();
    for (_i, data) in tokens.entries().enumerate() {
        if uid == data.1.as_i64().unwrap() {
            return data.0.to_string();
        }
    }
    String::new()
}
pub fn get_user_rank_data(exp: i64) -> JsonValue {
    let ranks = json::parse(include_str!("user_rank.json")).unwrap();
    
    for (i, rank) in ranks.members().enumerate() {
        if exp < rank["exp"].as_i64().unwrap() {
            return ranks[i - 1].clone();
        }
    }
    return ranks[ranks.len() - 1].clone();
}

fn get_data(auth_key: &str, row: &str) -> JsonValue {
    let key = get_key(&auth_key);
    
    let result = lock_and_select(&format!("SELECT {} FROM _{}_", row, key));
    
    json::parse(&result.unwrap()).unwrap()
}

pub fn get_acc(auth_key: &str) -> JsonValue {
    let mut user = get_data(auth_key, "userdata");
    user["gem"]["total"] = (user["gem"]["charge"].as_i64().unwrap() + user["gem"]["free"].as_i64().unwrap()).into();
    
    let max = get_user_rank_data(user["user"]["exp"].as_i64().unwrap())["maxLp"].as_u64().unwrap();
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

pub fn get_acc_home(auth_key: &str) -> JsonValue {
    get_data(auth_key, "userhome")
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

pub fn save_data(auth_key: &str, row: &str, data: JsonValue) {
    let key = get_key(&auth_key);
    
    lock_and_exec(&format!("UPDATE _{}_ SET {}=?1", key, row), params!(json::stringify(data)));
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

pub fn get_acc_transfer(uid: i64, token: &str, password: &str) -> JsonValue {
    create_migration_store();
    
    let result = lock_and_select("SELECT jsondata FROM migrationdata");
    let data = json::parse(&result.unwrap()).unwrap();
    
    if data[token].is_empty() {
        return object!{success: false};
    }
    if data[token].to_string() == password.to_string() {
        let login_token = get_login_token(uid);
        if login_token == String::new() {
            return object!{success: false};
        }
        return object!{success: true, login_token: login_token};
    }
    
    return object!{success: false};
}

pub fn save_acc_transfer(token: &str, password: &str) {
    create_migration_store();
    
    let result = lock_and_select("SELECT jsondata FROM migrationdata");
    let mut data = json::parse(&result.unwrap()).unwrap();
    
    data[token] = password.into();
    
    store_data("migrationdata", data);
}

pub fn get_name_and_rank(uid: i64) -> JsonValue {
    create_migration_store();
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
    let result = lock_and_select(&format!("SELECT userdata FROM _{}_", uid));
    let data = json::parse(&result.unwrap()).unwrap();
    
    return object!{
        user_name: data["user"]["name"].clone(),
        user_rank: get_user_rank_data(data["user"]["exp"].as_i64().unwrap())["rank"].clone() //todo
    }
}

pub fn get_acc_from_uid(uid: i64) -> JsonValue {
    create_migration_store();
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return object!{
            user_name: "",
            user_rank: 1
        }
    }
    let uid = get_uid(&login_token);
    if uid == 0 || !acc_exists(uid) {
        return object!{"error": true}
    }
    let result = lock_and_select(&format!("SELECT userdata FROM _{}_", uid));
    json::parse(&result.unwrap()).unwrap()
}
