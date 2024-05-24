use json::{JsonValue};
use actix_web::{HttpRequest};

use crate::encryption;

pub fn error(_req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    println!("client error: {}", body["code"]);
    
    None
}
