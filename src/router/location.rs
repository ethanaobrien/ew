use jzon::object;
use actix_web::{web, HttpRequest, Responder};

use crate::router::global;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/location", web::get().to(location));
}

async fn location(req: HttpRequest) -> impl Responder {
    global::api(&req, Some(object!{
        "master_location_ids": []
    }))
}
