use json::{object, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::global;
use crate::encryption;

pub fn error(req: HttpRequest, body: String) -> Option<JsonValue> {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    println!("client error: {}", body["code"]);
    
    None
}
