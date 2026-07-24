use jzon::{array};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{userdata, global, Session};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/tutorial", web::post().to(tutorial));
}

async fn tutorial(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    let mut user = userdata::get_acc(&key);

    if user["tutorial_step"].as_i32().unwrap() < 130 {
        user["tutorial_step"] = body["step"].clone();
        user["stamina"]["stamina"] = (100).into();
        user["stamina"]["last_updated_time"] = global::timestamp().into();
        userdata::save_acc(&key, user);
    }
    
    global::api(&req, Some(array![]))
}
