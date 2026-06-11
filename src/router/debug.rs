use jzon::object;
use actix_web::{web, HttpRequest, Responder};

use crate::encryption;
use crate::router::global;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/debug/error", web::post().to(error));
}

async fn error(req: HttpRequest, body: String) -> impl Responder {
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();

    println!("client error: {}", body["code"]);

    global::api(&req, Some(object!{}))
}
