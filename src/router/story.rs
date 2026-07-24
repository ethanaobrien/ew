use jzon::{object};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, userdata, databases, Session};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/story/read", web::post().to(read));
}

async fn read(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    let mut user = userdata::get_acc(&key);
    let part = body["master_story_part_id"].as_i64().unwrap();
    let master_id = databases::STORY[part.to_string()]["masterStoryId"].as_i64().unwrap();

    let index = user["story_list"].members().position(|r| r["master_story_id"] == master_id);

    if index.is_none() {
        user["story_list"].push(object!{
            master_story_id: master_id,
            master_story_part_ids: []
        }).unwrap();
    }

    for story in user["story_list"].members_mut() {
        if story["master_story_id"] == master_id && !story["master_story_part_ids"].contains(part) {
            story["master_story_part_ids"].push(part).unwrap();
        }
    }

    userdata::save_acc(&key, user.clone());


    global::api(&req, Some(object!{
        "gift_list":[],
        "updated_value_list":{
            "story_list": user["story_list"].clone()
        },
        "reward_list":[],
        "clear_mission_ids":[]
    }))
}
