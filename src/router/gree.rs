use crate::router::global;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue, http::header::ContentType, http::header::HeaderMap};
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use sha1::Sha1;
use substring::Substring;
use json::object;
use hmac::{Hmac, Mac};
use crate::router::userdata;
use crate::encryption;
use crate::router::user::{code_to_uid, uid_to_code};

use rusqlite::{Connection, params, ToSql};
use std::sync::{Mutex, MutexGuard};
use lazy_static::lazy_static;
use uuid::Uuid;

use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::hash::MessageDigest;
use openssl::sign::Verifier;

lazy_static! {
    pub static ref ENGINE: Mutex<Option<Connection>> = Mutex::new(None);
}
fn init(engine: &mut MutexGuard<'_, Option<Connection>>) {
    let conn = Connection::open("gree.db").unwrap();
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
fn uuid_exists(uuid: &str) -> bool {
    let data = lock_and_select("SELECT uuid FROM uuids WHERE uuid=?1", params!(uuid));
    data.is_ok()
}
fn get_new_uuid() -> String {
    create_store_v2("CREATE TABLE IF NOT EXISTS uuids (
        uuid  TEXT NOT NULL PRIMARY KEY
    )");
    let id = format!("{}", Uuid::new_v4());
    if uuid_exists(&id) {
        return get_new_uuid();
    }
    lock_and_exec("INSERT INTO uuids (uuid) VALUES (?1)", params!(&id));
    
    id
}
fn update_cert(uid: i64, cert: &str) {
    lock_and_exec("UPDATE users SET cert=?1 WHERE user_id=?2", params!(cert, uid));
}
fn create_acc(cert: &str) -> String {
    create_store_v2("CREATE TABLE IF NOT EXISTS users (
        cert     TEXT NOT NULL,
        uuid     TEXT NOT NULL,
        user_id  BIGINT NOT NULL PRIMARY KEY
    )");
    let uuid = get_new_uuid();
    let user = userdata::get_acc(&uuid);
    let user_id = user["user"]["id"].as_i64().unwrap();
    lock_and_exec(
        "INSERT INTO users (cert, uuid, user_id) VALUES (?1, ?2, ?3)",
        params!(cert, uuid, user_id)
    );
    
    uuid
}

fn verify_signature(signature: &[u8], message: &[u8], public_key: &[u8]) -> bool {
    let rsa_public_key = match Rsa::public_key_from_pem(public_key) {
        Ok(key) => key,
        Err(_) => return false,
    };
    let pkey = match PKey::from_rsa(rsa_public_key) {
        Ok(pkey) => pkey,
        Err(_) => return false,
    };
    let mut verifier = Verifier::new(MessageDigest::sha1(), &pkey).unwrap();
    verifier.update(message).unwrap();

    match verifier.verify(signature) {
        Ok(_) => true,
        Err(_) => false,
    }
}
pub fn get_uuid(headers: &HeaderMap, body: &str) -> String {
    let body = encryption::decrypt_packet(&body).unwrap();
    let blank_header = HeaderValue::from_static("");
    let login = headers.get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid = headers.get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("");
    let version = headers.get("aoharu-client-version").unwrap_or(&blank_header).to_str().unwrap_or("");
    let timestamp = headers.get("aoharu-timestamp").unwrap_or(&blank_header).to_str().unwrap_or("");
    if uid == "" || login == "" || version == "" || timestamp == "" {
        return String::new();
    }
    
    let cert = lock_and_select("SELECT cert FROM users WHERE user_id=?1;", params!(uid)).unwrap();
    
    let data = format!("{}{}{}{}{}", uid, "sk1bdzb310n0s9tl", version, timestamp, body);
    let encoded = general_purpose::STANDARD.encode(data.as_bytes());
    
    let decoded = general_purpose::STANDARD.decode(login).unwrap_or(vec![]);
    
    if verify_signature(&decoded, &encoded.as_bytes(), &cert.as_bytes()) {
        return lock_and_select("SELECT uuid FROM users WHERE user_id=?1;", params!(uid)).unwrap();
    } else {
        return String::new();
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
fn decrypt_transfer_password(password: &str) -> String {
    let reversed = password.chars().rev().collect::<String>();
    let rot = rot13(&reversed);
    let decoded = general_purpose::STANDARD.decode(rot).unwrap_or(vec![]);
    
    String::from_utf8_lossy(&decoded).to_string()
}



pub fn initialize(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&body).unwrap();
    let token = create_acc(&body["token"].to_string());
    
    let app_id = "232610769078541";
    let resp = object!{
        result: "OK",
        app_id: app_id,
        uuid: token
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization,Accept-Encoding"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn authorize(req: HttpRequest, _body: String) -> HttpResponse {
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization,Accept-Encoding"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn moderate_keyword(req: HttpRequest) -> HttpResponse {
    let resp = object!{
        result: "OK",
        entry: {
            timestamp: global::timestamp(),
            keywords: [{"id":"1","type":"0","keyword":"oink","rank":"0"}]
        }
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization,Accept-Encoding"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}
pub fn moderate_commit(req: HttpRequest, _body: String) -> HttpResponse {
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization,Accept-Encoding"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn uid(req: HttpRequest) -> HttpResponse {
    let mut uid = String::new();
    let blank_header = HeaderValue::from_static("");
    let auth_header = req.headers().get("Authorization").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid_data: Vec<&str> = auth_header.split(",xoauth_requestor_id=\"").collect();
    if let Some(uid_data2) = uid_data.get(1) {
        let uid_data2: Vec<&str> = uid_data2.split('"').collect();
        if let Some(uid_str) = uid_data2.get(0) {
            uid = uid_str.to_string();
        }
    }
    //println!("{}", uid);
    
    let user = userdata::get_acc(&uid);
    //println!("{}", user["user"]["id"].to_string());
    
    let resp = object!{
        result: "OK",
        x_uid: user["user"]["id"].to_string(),
        x_app_id: "100900301"
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn payment(req: HttpRequest) -> HttpResponse {
    let resp = object!{
        result: "OK",
        entry: {
            products :[],
            welcome: "0"
        }
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}
pub fn payment_ticket(req: HttpRequest) -> HttpResponse {
    let resp = object!{
        result: "OK",
        entry: []
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn migration_verify(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&body).unwrap();
    let password = decrypt_transfer_password(&body["migration_password"].to_string());
    
    let uid = code_to_uid(body["migration_code"].to_string()).parse::<i64>().unwrap_or(0);
    
    let user = userdata::get_acc_transfer(uid, &body["migration_code"].to_string(), &password);
    
    let resp;
    if user["success"].as_bool().unwrap() != true || uid == 0 {
        resp = object!{
            result: "ERR",
            messsage: "User Not Found"
        };
    } else {
        let data_user = userdata::get_acc(&user["login_token"].to_string());
        resp = object!{
            result: "OK",
            src_uuid: user["login_token"].clone(),
            src_x_uid: uid.to_string(),
            migration_token: user["login_token"].clone(),
            balance_charge_gem: data_user["gem"]["charge"].to_string(),
            balance_free_gem: data_user["gem"]["free"].to_string(),
            balance_total_gem: data_user["gem"]["total"].to_string()
        };
    }
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn migration(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&body).unwrap();
    
    let user = userdata::get_acc(&body["src_uuid"].to_string());
    //clear old token
    if !body["dst_uuid"].is_null() {
        let user2 = userdata::get_acc(&body["dst_uuid"].to_string());
        update_cert(user2["user"]["id"].as_i64().unwrap(), "none");
    }
    update_cert(user["user"]["id"].as_i64().unwrap(), &body["token"].to_string());
    
    let resp = object!{
        result: "OK"
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn balance(req: HttpRequest) -> HttpResponse {
    let mut uid = String::new();
    let blank_header = HeaderValue::from_static("");
    let auth_header = req.headers().get("Authorization").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid_data: Vec<&str> = auth_header.split(",xoauth_requestor_id=\"").collect();
    if let Some(uid_data2) = uid_data.get(1) {
        let uid_data2: Vec<&str> = uid_data2.split('"').collect();
        if let Some(uid_str) = uid_data2.get(0) {
            uid = uid_str.to_string();
        }
    }
    
    let user = userdata::get_acc(&uid);
    
    let resp = object!{
        result: "OK",
        entry: {
            balance_charge_gem: user["gem"]["charge"].to_string(),
            balance_free_gem: user["gem"]["free"].to_string(),
            balance_total_gem: user["gem"]["total"].to_string()
        }
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn migration_code(req: HttpRequest) -> HttpResponse {
    let mut uid = String::new();
    let blank_header = HeaderValue::from_static("");
    let auth_header = req.headers().get("Authorization").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid_data: Vec<&str> = auth_header.split(",xoauth_requestor_id=\"").collect();
    if let Some(uid_data2) = uid_data.get(1) {
        let uid_data2: Vec<&str> = uid_data2.split('"').collect();
        if let Some(uid_str) = uid_data2.get(0) {
            uid = uid_str.to_string();
        }
    }
    
    let user = userdata::get_acc(&uid);
    
    let resp = object!{
        result: "OK",
        migration_code: uid_to_code(user["user"]["id"].to_string())
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}

pub fn migration_password_register(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&body).unwrap();
    let mut uid = String::new();
    let blank_header = HeaderValue::from_static("");
    let auth_header = req.headers().get("Authorization").unwrap_or(&blank_header).to_str().unwrap_or("");
    let uid_data: Vec<&str> = auth_header.split(",xoauth_requestor_id=\"").collect();
    if let Some(uid_data2) = uid_data.get(1) {
        let uid_data2: Vec<&str> = uid_data2.split('"').collect();
        if let Some(uid_str) = uid_data2.get(0) {
            uid = uid_str.to_string();
        }
    }
    
    let user = userdata::get_acc(&uid);
    let code = uid_to_code(user["user"]["id"].to_string());
    let pass = decrypt_transfer_password(&body["migration_password"].to_string());
    
    userdata::save_acc_transfer(&code, &pass);
    
    let resp = object!{
        result: "OK"
    };
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body(json::stringify(resp))
}


fn gree_authorize(req: &HttpRequest) -> String {
    type HmacSha1 = Hmac<Sha1>;
    
    let blank_header = HeaderValue::from_static("");
    let auth_header = req.headers().get("Authorization").unwrap_or(&blank_header).to_str().unwrap_or("");
    if auth_header == "" {
        return String::new();
    }
    let auth_header = auth_header.substring(6, auth_header.len());

    let auth_list: Vec<&str> = auth_header.split(',').collect();
    let mut header_data = HashMap::new();

    for auth_data in auth_list {
        let data: Vec<&str> = auth_data.split('=').collect();
        if data.len() == 2 {
            header_data.insert(data[0].to_string(), data[1][1..(data[1].len() - 1)].to_string());
        }
    }
    
    let hostname = req.headers().get("host").unwrap_or(&blank_header).to_str().unwrap_or("");
    let current_url = format!("http://{}{}", hostname, req.path());
    let uri = req.uri().to_string();
    let extra = if uri.contains("?") {
        format!("&{}", uri.split('?').nth(1).unwrap_or(""))
    } else { String::new() };
    
    let nonce = format!("{:x}", md5::compute((global::timestamp() * 1000).to_string()));
    let timestamp = global::timestamp().to_string();
    let method = "HMAC-SHA1";
    let validate_data = format!("{}&{}&{}",
        req.method(),
        urlencoding::encode(&current_url),
        urlencoding::encode(&format!("oauth_body_hash={}&oauth_consumer_key={}&oauth_nonce={}&oauth_signature_method={}&oauth_timestamp={}&oauth_version=1.0{}", 
                                    header_data.get("oauth_body_hash").unwrap_or(&String::new()),
                                    header_data.get("oauth_consumer_key").unwrap_or(&String::new()),
                                    nonce,
                                    method,
                                    timestamp,
                                    extra)));
    let mut hasher = HmacSha1::new_from_slice(&hex::decode("6438663638653238346566646636306262616563326432323563306366643432").unwrap()).unwrap();
    hasher.update(validate_data.as_bytes());
    let signature = general_purpose::STANDARD.encode(hasher.finalize().into_bytes());

    format!("OAuth oauth_version=\"1.0\",oauth_nonce=\"{}\",oauth_timestamp=\"{}\",oauth_consumer_key=\"{}\",oauth_body_hash=\"{}\",oauth_signature_method=\"{}\",oauth_signature=\"{}\"", 
            nonce,
            timestamp,
            header_data.get("oauth_consumer_key").unwrap_or(&String::new()),
            header_data.get("oauth_body_hash").unwrap_or(&String::new()),
            method,
            urlencoding::encode(&signature))
}
