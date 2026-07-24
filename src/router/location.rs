use jzon::object;
use actix_web::{web, Responder};

use crate::router::Api;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/location", web::get().to(location));
}

async fn location() -> impl Responder {
    Api(Some(object!{
        "master_location_ids": []
    }))
}
