use actix_web::{HttpRequest, http::header::HeaderValue, web, Responder, HttpMessage};
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use actix_web::dev::{ServiceResponse, Service};
use actix_web::http::header;
use actix_web::http::header::HeaderName;
use sha1::Sha1;
use jzon::{object, JsonValue};
use hmac::{Hmac, Mac, KeyInit};

use crate::router::global;
use crate::router::userdata;

use crate::database::gree::*;

const APP_ID: &str = "232610769078541";
const SRC_APP_ID: &str = "100900301";
const HMAC_SECRET_HEX: &str = "6438663638653238346566646636306262616563326432323563306366643432";

struct RequireGreeAuth;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("")
            .wrap_fn(|req, srv| {
                let fut = srv.call(req);
                async move {
                    let mut res = fut.await?;
                    apply_headers(&mut res);
                    Ok(res)
                }
            })
            .service(
                web::scope("/auth")
                    .route("/x_uid", web::get().to(uid))
                    .route("/initialize", web::post().to(initialize))
                    .route("/authorize", web::post().to(authorize))
            )
            .service(
                web::scope("/payment")
                    .route("/productlist", web::get().to(payment))
                    .route("/balance", web::get().to(balance))
                    .route("/subscription/productlist", web::get().to(payment))
                    .route("/ticket/status", web::get().to(payment_ticket))
            )
            .service(
                web::scope("/moderate")
                    .route("/keywordlist", web::get().to(moderate_keyword))
                    .route("/filtering/commit", web::post().to(moderate_commit))
            )
            .service(
                web::scope("/migration")
                    .route("", web::post().to(migration))
                    .route("/code", web::get().to(migration_code))
                    .route("/code/verify", web::post().to(migration_verify))
                    .route("/password/register", web::post().to(migration_password_register))
            )
            .default_service(web::route().to(not_found))
    );
}

fn apply_headers(res: &mut ServiceResponse) {
    let gree_auth = res.request().extensions().get::<RequireGreeAuth>().is_some();
    let req = res.request().clone();
    let headers = res.headers_mut();

    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    headers.insert(header::EXPIRES, "-1".parse().unwrap());
    headers.insert(header::PRAGMA, "no-cache".parse().unwrap());
    headers.insert(
        header::CACHE_CONTROL,
        "must-revalidate, no-cache, no-store, private".parse().unwrap()
    );
    headers.insert(header::VARY, "Authorization,Accept-Encoding".parse().unwrap());

    if gree_auth {
        let auth_str = gree_authorize(&req);
        headers.insert(HeaderName::from_static("x-gree-authorization"), auth_str.parse().unwrap());
    }
}


fn get_uid(req: &HttpRequest) -> String {
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|auth| auth.split(",xoauth_requestor_id=\"").nth(1))
        .and_then(|s| s.split('"').next())
        .unwrap_or("")
        .to_string()
}

fn send(req: HttpRequest, resp: JsonValue) -> impl Responder {
    req.extensions_mut().insert(RequireGreeAuth);
    jzon::stringify(resp)
}

async fn not_found() -> impl Responder {
    let resp = object!{
        code: 10001,
        message: "Not Found",
        result: "NG"
    };
    jzon::stringify(resp)
}

async fn initialize(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&body).unwrap();
    let token = create_acc(&body["token"].to_string());

    let app_id = APP_ID;
    let resp = object!{
        result: "OK",
        app_id: app_id,
        uuid: token
    };

    send(req, resp)
}

async fn authorize(req: HttpRequest, _body: String) -> impl Responder {
    let resp = object!{
        result: "OK"
    };

    send(req, resp)
}

async fn moderate_keyword(req: HttpRequest) -> impl Responder {
    let resp = object!{
        result: "OK",
        entry: {
            timestamp: global::timestamp(),
            keywords: [{"id":"1","type":"0","keyword":"oink","rank":"0"}]
        }
    };

    send(req, resp)
}
async fn moderate_commit(req: HttpRequest, _body: String) -> impl Responder {
    let resp = object!{
        result: "OK"
    };

    send(req, resp)
}

async fn uid(req: HttpRequest) -> impl Responder {
    let uid = get_uid(&req);

    let user = userdata::get_acc(&uid);

    let resp = object!{
        result: "OK",
        x_uid: user["user"]["id"].to_string(),
        x_app_id: SRC_APP_ID
    };

    send(req, resp)
}

async fn payment(req: HttpRequest) -> impl Responder {
    let resp = object!{
        result: "OK",
        entry: {
            products :[],
            welcome: "0"
        }
    };

    send(req, resp)
}
async fn payment_ticket(req: HttpRequest) -> impl Responder {
    let resp = object!{
        result: "OK",
        entry: []
    };

    send(req, resp)
}

async fn migration_verify(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&body).unwrap();
    let password = decrypt_transfer_password(&body["migration_password"].to_string());

    let user = userdata::user::migration::get_acc_transfer(&body["migration_code"].to_string(), &password);

    let resp = if !user["success"].as_bool().unwrap() || user["user_id"] == 0 {
        object!{
            result: "ERR",
            messsage: "User Not Found"
        }
    } else {
        let data_user = userdata::get_acc(&user["login_token"].to_string());
        object!{
            result: "OK",
            src_uuid: user["login_token"].clone(),
            src_x_uid: user["user_id"].to_string(),
            migration_token: user["login_token"].clone(),
            balance_charge_gem: data_user["gem"]["charge"].to_string(),
            balance_free_gem: data_user["gem"]["free"].to_string(),
            balance_total_gem: data_user["gem"]["total"].to_string()
        }
    };

    send(req, resp)
}

async fn migration(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&body).unwrap();

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

    send(req, resp)
}

async fn balance(req: HttpRequest) -> impl Responder {
    let uid = get_uid(&req);

    let user = userdata::get_acc(&uid);

    let resp = object!{
        result: "OK",
        entry: {
            balance_charge_gem: user["gem"]["charge"].to_string(),
            balance_free_gem: user["gem"]["free"].to_string(),
            balance_total_gem: user["gem"]["total"].to_string()
        }
    };

    send(req, resp)
}

async fn migration_code(req: HttpRequest) -> impl Responder {
    let uid = get_uid(&req);

    let user = userdata::get_acc(&uid);

    let resp = object!{
        result: "OK",
        migration_code: userdata::user::migration::get_acc_token(user["user"]["id"].as_i64().unwrap())
    };

    send(req, resp)
}

async fn migration_password_register(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&body).unwrap();
    let uid = get_uid(&req);
    let user = userdata::get_acc(&uid);
    let pass = decrypt_transfer_password(&body["migration_password"].to_string());
    
    userdata::user::migration::save_acc_transfer(user["user"]["id"].as_i64().unwrap(), &pass);
    
    let resp = object!{
        result: "OK"
    };
    
    send(req, resp)
}

pub fn get_protocol() -> String {
    let args = crate::get_args();
    if args.https {
        return String::from("https");
    }
    String::from("http")
}

fn gree_authorize(req: &HttpRequest) -> String {
    type HmacSha1 = Hmac<Sha1>;
    
    let blank_header = HeaderValue::from_static("");
    let auth_header = req.headers().get("Authorization").unwrap_or(&blank_header).to_str().unwrap_or("");
    if auth_header.is_empty() {
        return String::new();
    }
    let auth_header = auth_header.get(6..).unwrap_or("");

    let auth_list: Vec<&str> = auth_header.split(',').collect();
    let mut header_data = HashMap::new();

    for auth_data in auth_list {
        let data: Vec<&str> = auth_data.split('=').collect();
        if data.len() == 2 {
            header_data.insert(data[0].to_string(), data[1][1..(data[1].len() - 1)].to_string());
        }
    }
    
    let hostname = req.headers().get("host").unwrap_or(&blank_header).to_str().unwrap_or("");
    let current_url = format!("{}://{}{}", get_protocol(), hostname, req.path());
    let uri = req.uri().to_string();
    let extra = if uri.contains('?') {
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
    let mut hasher = HmacSha1::new_from_slice(&hex::decode(HMAC_SECRET_HEX).unwrap()).unwrap();
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
