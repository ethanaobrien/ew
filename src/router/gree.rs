use crate::router::global;
use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue, http::header::ContentType};
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use sha1::Sha1;
use substring::Substring;

pub fn initialize(req: HttpRequest, _body: String) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Expires", "-1"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Cache-Control", "must-revalidate, no-cache, no-store, private"))
        .insert_header(("Vary", "Authorization,Accept-Encoding"))
        .insert_header(("X-GREE-Authorization", gree_authorize(&req)))
        .body("{\"result\": \"OK\"}")
    
}

use hmac::{Hmac, Mac};


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
    
    
    let current_url = format!("http://127.0.0.1:8080{}", req.path());
    let nonce = format!("{:x}", md5::compute((global::timestamp() * 1000).to_string()));
    let timestamp = global::timestamp().to_string();
    let method = "HMAC-SHA1";
    let validate_data = format!("{}&{}&{}", 
        "POST",
        urlencoding::encode(&current_url),
        urlencoding::encode(&format!("oauth_body_hash={}&oauth_consumer_key={}&oauth_nonce={}&oauth_signature_method={}&oauth_timestamp={}&oauth_version=1.0", 
                                    header_data.get("oauth_body_hash").unwrap_or(&String::new()),
                                    header_data.get("oauth_consumer_key").unwrap_or(&String::new()),
                                    nonce,
                                    method,
                                    timestamp)));
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
