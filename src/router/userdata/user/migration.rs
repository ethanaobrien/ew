use rusqlite::params;
use json::{JsonValue, object};
use crate::router::userdata;
use rand::Rng;
use sha2::{Digest, Sha256};
use base64::{Engine as _, engine::general_purpose};

fn generate_token() -> String {
    let charset = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    let random_string: String = (0..16)
        .map(|_| {
            let idx = rng.random_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();
    random_string
}

pub fn get_acc_transfer(token: &str, password: &str) -> JsonValue {
    let database = userdata::get_userdata_database();
    let data = database.lock_and_select("SELECT password FROM migration WHERE token=?1", params!(token));
    if data.is_err() {
        return object!{success: false};
    }
    if verify_password(password, &data.unwrap()) {
        let uid: i64 = database.lock_and_select_type("SELECT user_id FROM migration WHERE token=?1", params!(token)).unwrap();
        let login_token = userdata::get_login_token(uid);
        if login_token == String::new() {
            return object!{success: false};
        }
        return object!{success: true, login_token: login_token, user_id: uid};
    }
    object!{success: false}
}

pub fn save_acc_transfer(uid: i64, password: &str) -> String {
    let database = userdata::get_userdata_database();
    let token = if let Ok(value) = database.lock_and_select("SELECT token FROM migration WHERE user_id=?1", params!(uid)) {
        value
    } else {
        generate_token()
    };
    database.lock_and_exec("DELETE FROM migration WHERE user_id=?1", params!(uid));
    database.lock_and_exec("INSERT INTO migration (user_id, token, password) VALUES (?1, ?2, ?3)", params!(uid, &token, hash_password(password)));
    token
}

pub fn get_acc_token(uid: i64) -> String {
    let database = userdata::get_userdata_database();
    if let Ok(value) = database.lock_and_select("SELECT token FROM migration WHERE user_id=?1", params!(uid)) {
        value
    } else {
        save_acc_transfer(uid, "")
    }
}

fn generate_salt() -> Vec<u8> {
    let mut rng = rand::rng();
    let mut bytes = vec![0u8; 16];
    rng.fill(&mut bytes[..]);
    bytes
}

fn hash_password(password: &str) -> String {
    if password.is_empty() { return String::new(); };
    let salt = &generate_salt();
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt);
    let hashed_password = hasher.finalize();

    let salt_hash = [&salt[..], &hashed_password[..]].concat();
    general_purpose::STANDARD.encode(salt_hash)
}

pub fn verify_password(password: &str, salted_hash: &str) -> bool {
    if password.is_empty() || salted_hash.is_empty() {
        return false;
    }
    let bytes = general_purpose::STANDARD.decode(salted_hash);
    if bytes.is_err() {
        return password == salted_hash;
    }
    let bytes = bytes.unwrap();
    if bytes.len() < 17 {
        return password == salted_hash;
    }
    let (salt, hashed_password) = bytes.split_at(16);
    let hashed_password = &hashed_password[0..32];

    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt);
    let input_hash = hasher.finalize();

    input_hash.as_slice() == hashed_password
}

pub fn setup_sql(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute("
    CREATE TABLE IF NOT EXISTS migration (
        user_id           BIGINT NOT NULL,
        token             TEXT NOT NULL,
        password          TEXT NOT NULL,
        PRIMARY KEY (user_id, token)
    );
    ", [])?;
    let is_updated = conn.prepare("SELECT user_id FROM migration LIMIT 1;").is_ok();
    if is_updated { return Ok(()); }
    println!("Upgrading migration table");
    conn.execute("DROP TABLE IF EXISTS migration_new;", [])?;
    conn.execute("
    CREATE TABLE migration_new (
        user_id           BIGINT NOT NULL,
        token             TEXT NOT NULL,
        password          TEXT NOT NULL,
        PRIMARY KEY (user_id, token)
    );
    ", [])?;
    
    let mut stmt = conn.prepare("SELECT token, password FROM migration")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
        ))
    })?;

    let mut insert_new_row = conn.prepare("INSERT INTO migration_new (user_id, token, password) VALUES (?1, ?2, ?3)")?;
    for row in rows {
        let (token, password) = row?;

        let user_id = code_to_uid(&token);

        insert_new_row.execute(params![user_id, token, password])?;
    }
    conn.execute("DROP TABLE migration;", params!())?;
    conn.execute("ALTER TABLE migration_new RENAME TO migration;", params!())?;
    Ok(())
}

fn code_to_uid(code: &str) -> String {
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
