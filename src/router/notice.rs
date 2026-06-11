use jzon::{object, array};
use actix_web::{web, HttpRequest, Responder};

use crate::router::global;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/notice/reward").route(web::get().to(reward)).route(web::post().to(reward_post)));
}

//todo
async fn reward(req: HttpRequest) -> impl Responder {
    global::api(&req, Some(object!{
        "reward_list": []
    }))
}

async fn reward_post(req: HttpRequest, _body: String) -> impl Responder {
    global::api(&req, Some(array![]))
}
