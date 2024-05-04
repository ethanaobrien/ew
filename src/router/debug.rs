use json::object;
use actix_web::{HttpResponse, HttpRequest};

use crate::router::global;
use crate::encryption;

pub fn error(req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    println!("client error: {}", body["code"]);
    
    let resp = object!{
        "code": 2,
        "server_time": global::timestamp(),
        "message": ""
    };
    global::send(resp, req)
}
