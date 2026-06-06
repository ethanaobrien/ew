use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2
};
use rusqlite::params;
use jzon::{JsonValue, object};
use crate::router::userdata;
use rand::RngExt;
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

fn hash_password(password: &str) -> String {
    if password.is_empty() { return String::new(); }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    if password.is_empty() || hash.is_empty() {
        return false;
    }
    if !hash.starts_with("$argon2") {
        return legacy_verify_password(password, hash);
    }
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
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

// For migrating from a legacy database
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

fn legacy_verify_password(password: &str, salted_hash: &str) -> bool {
    let bytes = match general_purpose::STANDARD.decode(salted_hash) {
        Ok(b) if b.len() >= 17 => b,
        _ => return password == salted_hash,
    };
    let (salt, hashed_password) = bytes.split_at(16);
    let hashed_password = &hashed_password[0..32];
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt);
    let input_hash = hasher.finalize();
    input_hash.as_slice() == hashed_password
}
