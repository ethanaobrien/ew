use jzon::{object, array};
use actix_web::{web, Responder};

use crate::router::Api;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/notice/reward").route(web::get().to(reward)).route(web::post().to(reward_post)));
}

//todo
async fn reward() -> impl Responder {
    Api(Some(object!{
        "reward_list": []
    }))
}

async fn reward_post() -> impl Responder {
    Api(Some(array![]))
}
