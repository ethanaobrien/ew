use jzon::object;
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, Body};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/debug/error", web::post().to(error));
}

async fn error(req: HttpRequest, Body(body): Body) -> impl Responder {

    println!("client error: {}", body["code"]);

    global::api(&req, Some(object!{}))
}
