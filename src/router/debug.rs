use jzon::object;
use actix_web::{web, Responder};

use crate::router::{Body, Api};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/debug/error", web::post().to(error));
}

async fn error(Body(body): Body) -> impl Responder {

    println!("client error: {}", body["code"]);

    Api(Some(object!{}))
}
