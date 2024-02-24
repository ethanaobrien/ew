use rusqlite::{Connection, params};
use std::sync::{Mutex, MutexGuard};
use lazy_static::lazy_static;
use json::{JsonValue, array, object};
//use base64::{Engine as _, engine::general_purpose};

lazy_static! {
    pub static ref ENGINE: Mutex<Option<Connection>> = Mutex::new(None);
}

fn init(engine: &mut MutexGuard<'_, Option<Connection>>) {
    let conn = Connection::open("userdata.db").unwrap();
    conn.execute("PRAGMA foreign_keys = ON;", ()).unwrap();

    engine.replace(conn);
}
fn create_uid_store(conn: &Connection) {
    match conn.prepare("SELECT jsondata FROM uids") {
        Ok(_) => {}
        Err(_) => {
            conn.execute(
                "CREATE TABLE uids (
                    jsondata  TEXT NOT NULL
                )",
                (), // empty list of parameters.
            ).unwrap();
            init_data(conn, "uids", array![]);
        }
    }
    store_data(conn, "uids", array![]);
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

fn create_acc(conn: &Connection, uid: i64) {
    let key = &uid.to_string();
    conn.execute(
        &format!("CREATE TABLE _{}_ (
            jsondata  TEXT NOT NULL
        )", key),
        (),
    ).unwrap();
    let mut data = object!{
        userdata: json::parse(include_str!("new_user.json")).unwrap()
    };
    data["userdata"]["user"]["id"] = uid.into();
    
    init_data(conn, &format!("_{}_", key), data);
}

//a6573cbe is the name of the header - todo - more secure than just uid
pub fn get_acc(_a6573cbe: &str, uid: &str) -> JsonValue {
    //let decoded = general_purpose::STANDARD.decode(a6573cbe).unwrap();
    //let header = String::from_utf8_lossy(&decoded);
    
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                
                let key: i64;
                /*
                if header.starts_with("0") {
                    key = generate_uid(conn);
                    create_acc(conn, key);
                } else {
                    key = header[..15].parse::<i64>().unwrap();//.unwrap_or(generate_uid(conn));
                }*/
                if uid == "" {
                    key = generate_uid(conn);
                    create_acc(conn, key);
                } else {
                    key = uid.parse::<i64>().unwrap();
                }
                
                if !acc_exists(conn, key) {
                    create_acc(conn, key);
                }
                let mut stmt = conn.prepare(&format!("SELECT jsondata FROM _{}_", key)).unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let rv = json::parse(&result.unwrap()).unwrap();
                
                return rv["userdata"].clone();
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}

pub fn save_acc(_a6573cbe: &str, uid: &str, data: JsonValue) {
    //let decoded = general_purpose::STANDARD.decode(a6573cbe).unwrap();
    //let header = String::from_utf8_lossy(&decoded);
    
    loop {
        match ENGINE.lock() {
            Ok(mut result) => {
                if result.is_none() {
                    init(&mut result);
                }
                let conn = result.as_ref().unwrap();
                
                let key: i64;
                /*
                if header.starts_with("0") {
                    key = generate_uid(conn);
                    create_acc(conn, key);
                } else {
                    key = header[..15].parse::<i64>().unwrap();//.unwrap_or(generate_uid(conn));
                }*/
                if uid == "" {
                    key = generate_uid(conn);
                    create_acc(conn, key);
                } else {
                    key = uid.parse::<i64>().unwrap();
                }
                
                if !acc_exists(conn, key) {
                    create_acc(conn, key);
                }
                let mut stmt = conn.prepare(&format!("SELECT jsondata FROM _{}_", key)).unwrap();
                let result: Result<String, rusqlite::Error> = stmt.query_row([], |row| row.get(0));
                
                let mut rv = json::parse(&result.unwrap()).unwrap();
                
                rv["userdata"] = data;
                store_data(conn, &format!("_{}_", key), rv);
                break;
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }
}
