use jzon::{array};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{userdata, global};
use crate::encryption;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/tutorial", web::post().to(tutorial));
}

async fn tutorial(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);

    if user["tutorial_step"].as_i32().unwrap() < 130 {
        user["tutorial_step"] = body["step"].clone();
        user["stamina"]["stamina"] = (100).into();
        user["stamina"]["last_updated_time"] = global::set_time(global::timestamp(), user["user"]["id"].as_i64().unwrap(), false).into();
        userdata::save_acc(&key, user);
    }
    
    global::api(&req, Some(array![]))
}
