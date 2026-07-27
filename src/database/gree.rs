use actix_web::http::header::{HeaderMap, HeaderValue};
use base64::{Engine as _, engine::general_purpose};
use lazy_static::lazy_static;
use rsa::{Pkcs1v15Sign, RsaPublicKey};
use rusqlite::params;
use sha1::Sha1;
use sha1::Digest;
use rsa::pkcs8::DecodePublicKey;

use crate::router::{global, userdata};
use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("gree.db", setup_tables);
}

pub fn setup() {
    vacuum_database();
}

fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS users (
        cert     TEXT NOT NULL,
        uuid     TEXT NOT NULL,
        user_id  BIGINT NOT NULL PRIMARY KEY
    );").unwrap();
}

pub fn update_cert(uid: i64, cert: &str) {
    if DATABASE.lock_and_select("SELECT cert FROM users WHERE user_id=?1;", params!(uid)).is_err() {
        let token = userdata::get_login_token(uid);
        if token != String::new() {
            DATABASE.lock_and_exec(
                "INSERT INTO users (cert, uuid, user_id) VALUES (?1, ?2, ?3)",
                params!(cert, token, uid)
            );
            return;
        }
    }
    DATABASE.lock_and_exec("UPDATE users SET cert=?1 WHERE user_id=?2", params!(cert, uid));
}

pub fn create_acc(cert: &str) -> String {
    let uuid = global::create_token();
    let user = userdata::get_acc(&uuid);
    let user_id = user["user"]["id"].as_i64().unwrap();
    DATABASE.lock_and_exec(
        "INSERT INTO users (cert, uuid, user_id) VALUES (?1, ?2, ?3)",
        params!(cert, uuid, user_id)
    );

    uuid
}

pub fn delete_uuid(user_id: i64) {
    DATABASE.lock_and_exec("DELETE FROM users WHERE user_id=?1", params!(user_id));
}

fn vacuum_database() {
    DATABASE.lock_and_exec("VACUUM", params!());
}

pub fn is_registered(uuid: &str) -> bool {
    match DATABASE.lock_and_select("SELECT cert FROM users WHERE uuid=?1;", params!(uuid)) {
        Ok(cert) => cert != "none",
        Err(_) => false
    }
}

fn verify_signature(signature: &[u8], message: &[u8], public_key: &str) -> bool {
    let Ok(pem) = pem::parse(public_key) else {
        return false;
    };
    let Ok(public_key) = RsaPublicKey::from_public_key_der(&pem.contents()) else {
        return false;
    };
    let digest = Sha1::digest(message);

    public_key
        .verify(Pkcs1v15Sign::new::<Sha1>(), &digest, signature)
        .is_ok()
}

pub fn get_uuid(headers: &HeaderMap, body: &str) -> String {
    let blank_header = HeaderValue::from_static("");
    let login = headers.get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = headers.get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let version = headers.get("aoharu-client-version").unwrap_or(&blank_header).to_str().unwrap_or("");
    let timestamp = headers.get("aoharu-timestamp").unwrap_or(&blank_header).to_str().unwrap_or("");
    if uid.is_empty() || login.is_empty() || version.is_empty() || timestamp.is_empty() {
        return String::new();
    }

    let Ok(cert) = DATABASE.lock_and_select("SELECT cert FROM users WHERE user_id=?1;", params!(uid)) else {
        return String::new();
    };

    let data = format!("{}{}{}{}{}", uid, "sk1bdzb310n0s9tl", version, timestamp, body);
    let encoded = general_purpose::STANDARD.encode(data.as_bytes());

    let decoded = general_purpose::STANDARD.decode(login).unwrap_or_default();

    if verify_signature(&decoded, encoded.as_bytes(), &cert) {
        DATABASE.lock_and_select("SELECT uuid FROM users WHERE user_id=?1;", params!(uid)).unwrap()
    } else {
        String::new()
    }
}

fn rot13(input: &str) -> String {
    let mut result = String::new();
    for c in input.chars() {
        let shifted = match c {
            'A'..='Z' => ((c as u8 - b'A' + 13) % 26 + b'A') as char,
            'a'..='z' => ((c as u8 - b'a' + 13) % 26 + b'a') as char,
            _ => c,
        };
        result.push(shifted);
    }
    result
}
pub fn decrypt_transfer_password(password: &str) -> String {
    let reversed = password.chars().rev().collect::<String>();
    let rot = rot13(&reversed);
    let decoded = general_purpose::STANDARD.decode(rot).unwrap_or_default();

    String::from_utf8_lossy(&decoded).to_string()
}
