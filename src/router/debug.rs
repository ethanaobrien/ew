use json;
use json::{object};
use crate::router::global;
use crate::encryption;
use actix_web::{HttpResponse, HttpRequest};

pub fn error(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    println!("client error: {}", body["code"]);
    
    let resp = object!{
        "code": 2,
        "server_time": global::timestamp(),
        "message": ""
    };
    global::send(resp)
}
