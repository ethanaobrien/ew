use rusqlite::params;
use json::{JsonValue, object};
use crate::router::userdata;
use rand::Rng;
use sha2::{Digest, Sha256};
use base64::{Engine as _, engine::general_purpose};

pub fn get_acc_transfer(uid: i64, token: &str, password: &str) -> JsonValue {
    let database = userdata::get_userdata_database();
    let data = database.lock_and_select("SELECT password FROM migration WHERE token=?1", params!(token));
    if data.is_err() {
        return object!{success: false};
    }
    if verify_password(password, &data.unwrap()) {
        let login_token = userdata::get_login_token(uid);
        if login_token == String::new() {
            return object!{success: false};
        }
        return object!{success: true, login_token: login_token};
    }
    object!{success: false}
}

pub fn save_acc_transfer(token: &str, password: &str) {
    let database = userdata::get_userdata_database();
    database.lock_and_exec("DELETE FROM migration WHERE token=?1", params!(token));
    database.lock_and_exec("INSERT INTO migration (token, password) VALUES (?1, ?2)", params!(token, hash_password(password)));
}

fn generate_salt() -> Vec<u8> {
    let mut rng = rand::rng();
    let mut bytes = vec![0u8; 16];
    rng.fill(&mut bytes[..]);
    bytes
}

fn hash_password(password: &str) -> String {
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
